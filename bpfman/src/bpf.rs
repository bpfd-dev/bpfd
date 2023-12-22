// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of bpfman

use std::{
    collections::HashMap,
    convert::TryInto,
    fs::{create_dir_all, read_dir, remove_dir_all},
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use aya::{
    programs::{
        kprobe::KProbeLink, links::FdLink, loaded_programs, trace_point::TracePointLink,
        uprobe::UProbeLink, KProbe, TracePoint, UProbe,
    },
    BpfLoader,
};
use bpfman_api::{
    config::Config,
    util::directories::*,
    ProbeType::{self, *},
    ProgramType,
};
use log::{debug, info};
use tokio::{select, sync::mpsc::Receiver};

use crate::{
    command::{
        BpfMap, Command, Direction,
        Direction::{Egress, Ingress},
        Program, ProgramData, PullBytecodeArgs,
    },
    errors::BpfmanError,
    multiprog::{Dispatcher, DispatcherId, DispatcherInfo, TcDispatcher, XdpDispatcher},
    oci_utils::image_manager::IMAGE_MANAGER,
    serve::shutdown_handler,
    utils::{bytes_to_string, get_ifindex, set_dir_permissions, should_map_be_pinned},
    ROOT_DB,
};

const MAPS_MODE: u32 = 0o0660;

pub(crate) struct BpfManager {
    config: Config,
    dispatchers: DispatcherMap,
    programs: ProgramMap,
    maps: HashMap<u32, BpfMap>,
    commands: Receiver<Command>,
}

pub(crate) struct ProgramMap {
    programs: HashMap<u32, Program>,
}

impl ProgramMap {
    fn new() -> Self {
        ProgramMap {
            programs: HashMap::new(),
        }
    }

    fn insert(&mut self, id: u32, prog: Program) -> Option<Program> {
        self.programs.insert(id, prog)
    }

    fn remove(&mut self, id: &u32) -> Option<Program> {
        self.programs.remove(id)
    }

    fn get_mut(&mut self, id: &u32) -> Option<&mut Program> {
        self.programs.get_mut(id)
    }

    fn get(&self, id: &u32) -> Option<&Program> {
        self.programs.get(id)
    }

    fn programs_mut<'a>(
        &'a mut self,
        program_type: &'a ProgramType,
        if_index: &'a Option<u32>,
        direction: &'a Option<Direction>,
    ) -> impl Iterator<Item = &'a mut Program> {
        self.programs.values_mut().filter(|p| {
            p.kind() == *program_type
                && p.if_index().unwrap() == *if_index
                && p.direction().unwrap() == *direction
        })
    }

    // Adds a new program and sets the positions of programs that are to be attached via a dispatcher.
    // Positions are set based on order of priority. Ties are broken based on:
    // - Already attached programs are preferred
    // - Program name. Lowest lexical order wins.
    fn add_and_set_program_positions(&mut self, program: &mut Program) {
        let program_type = program.kind();
        let if_index = program.if_index().unwrap();
        let direction = program.direction().unwrap();

        let mut extensions = self
            .programs
            .values_mut()
            .filter(|p| {
                p.kind() == program_type
                    && p.if_index().unwrap() == if_index
                    && p.direction().unwrap() == direction
            })
            .collect::<Vec<&mut Program>>();

        // add program we're loading
        extensions.push(program);

        extensions.sort_by_key(|b| {
            (
                b.priority().unwrap(),
                b.attached(),
                b.get_data().get_name().unwrap().to_owned(),
            )
        });
        for (i, v) in extensions.iter_mut().enumerate() {
            v.set_position(i).expect("unable to set program position");
        }
    }

    // Sets the positions of programs that are to be attached via a dispatcher.
    // Positions are set based on order of priority. Ties are broken based on:
    // - Already attached programs are preferred
    // - Program name. Lowest lexical order wins.
    fn set_program_positions(
        &mut self,
        program_type: ProgramType,
        if_index: u32,
        direction: Option<Direction>,
    ) {
        let mut extensions = self
            .programs
            .values_mut()
            .filter(|p| {
                p.kind() == program_type
                    && p.if_index().unwrap() == Some(if_index)
                    && p.direction().unwrap() == direction
            })
            .collect::<Vec<&mut Program>>();

        extensions.sort_by_key(|b| {
            (
                b.priority().unwrap(),
                b.attached(),
                b.get_data().get_name().unwrap().to_owned(),
            )
        });
        for (i, v) in extensions.iter_mut().enumerate() {
            v.set_position(i).expect("unable to set program position");
        }
    }

    fn get_programs_iter(&self) -> impl Iterator<Item = (u32, &Program)> {
        self.programs
            .values()
            .map(|p| (p.get_data().get_id().unwrap(), p))
    }
}

pub(crate) struct DispatcherMap {
    dispatchers: HashMap<DispatcherId, Dispatcher>,
}

impl DispatcherMap {
    fn new() -> Self {
        DispatcherMap {
            dispatchers: HashMap::new(),
        }
    }

    fn remove(&mut self, id: &DispatcherId) -> Option<Dispatcher> {
        self.dispatchers.remove(id)
    }

    fn insert(&mut self, id: DispatcherId, dis: Dispatcher) -> Option<Dispatcher> {
        self.dispatchers.insert(id, dis)
    }

    /// Returns the number of extension programs currently attached to the dispatcher that
    /// would be used to attach the provided [`Program`].
    fn attached_programs(&self, did: &DispatcherId) -> usize {
        if let Some(d) = self.dispatchers.get(did) {
            d.num_extensions()
        } else {
            0
        }
    }
}

impl BpfManager {
    pub(crate) fn new(config: Config, commands: Receiver<Command>) -> Self {
        Self {
            config,
            dispatchers: DispatcherMap::new(),
            programs: ProgramMap::new(),
            maps: HashMap::new(),
            commands,
        }
    }

    fn allow_unsigned(&self) -> bool {
        self.config
            .signing
            .as_ref()
            .map_or(true, |s| s.allow_unsigned)
    }

    pub(crate) fn rebuild_state(&mut self) -> Result<(), anyhow::Error> {
        debug!("BpfManager::rebuild_state()");

        // re-build programs from database
        for tree_name in ROOT_DB.tree_names() {
            let name = &bytes_to_string(&tree_name);
            let tree = ROOT_DB
                .open_tree(name)
                .expect("unable to open database tree");

            let id = match name.parse::<u32>() {
                Ok(id) => id,
                Err(_) => {
                    debug!("Ignoring non-numeric tree name: {} on rebuild", name);
                    continue;
                }
            };

            debug!("rebuilding state for program {}", id);

            // If there's an error here remove broken tree and continue
            match Program::new_from_db(id, tree) {
                Ok(mut program) => {
                    program
                        .get_data_mut()
                        .set_program_bytes(self.allow_unsigned())?;
                    self.rebuild_map_entry(id, &mut program);
                    self.programs.insert(id, program);
                }
                Err(_) => {
                    ROOT_DB
                        .drop_tree(name)
                        .expect("unable to remove broken program tree");
                }
            }
        }

        self.rebuild_dispatcher_state(ProgramType::Xdp, None, RTDIR_XDP_DISPATCHER)?;
        self.rebuild_dispatcher_state(ProgramType::Tc, Some(Ingress), RTDIR_TC_INGRESS_DISPATCHER)?;
        self.rebuild_dispatcher_state(ProgramType::Tc, Some(Egress), RTDIR_TC_EGRESS_DISPATCHER)?;

        Ok(())
    }

    pub(crate) fn rebuild_dispatcher_state(
        &mut self,
        program_type: ProgramType,
        direction: Option<Direction>,
        path: &str,
    ) -> Result<(), anyhow::Error> {
        read_dir(path)?;
        for entry in read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let parts: Vec<&str> = name.to_str().unwrap().split('_').collect();
            if parts.len() != 2 {
                continue;
            }
            let if_index: u32 = parts[0].parse().unwrap();
            let revision: u32 = parts[1].parse().unwrap();
            match program_type {
                ProgramType::Xdp => {
                    let dispatcher = XdpDispatcher::load(if_index, revision).unwrap();
                    self.dispatchers.insert(
                        DispatcherId::Xdp(DispatcherInfo(if_index, None)),
                        Dispatcher::Xdp(dispatcher),
                    );
                }
                ProgramType::Tc => {
                    let direction = direction.expect("direction required for tc programs");

                    let dispatcher = TcDispatcher::load(if_index, direction, revision).unwrap();
                    let did = DispatcherId::Tc(DispatcherInfo(if_index, Some(direction)));

                    self.dispatchers.insert(
                        DispatcherId::Tc(DispatcherInfo(if_index, Some(direction))),
                        Dispatcher::Tc(dispatcher),
                    );

                    self.rebuild_multiattach_dispatcher(
                        did,
                        if_index,
                        ProgramType::Tc,
                        Some(direction),
                    )?;
                }
                _ => return Err(anyhow!("invalid program type {:?}", program_type)),
            }
        }

        Ok(())
    }

    pub(crate) fn add_program(&mut self, mut program: Program) -> Result<Program, BpfmanError> {
        let map_owner_id = program.get_data().get_map_owner_id()?;
        // Set map_pin_path if we're using another program's maps
        if let Some(map_owner_id) = map_owner_id {
            let map_pin_path = self.is_map_owner_id_valid(map_owner_id)?;
            program.get_data_mut().set_map_pin_path(&map_pin_path)?;
        }

        program
            .get_data_mut()
            .set_program_bytes(self.allow_unsigned())?;

        let result = match program {
            Program::Xdp(_) | Program::Tc(_) => {
                program.set_if_index(get_ifindex(&program.if_name().unwrap())?)?;

                self.add_multi_attach_program(&mut program)
            }
            Program::Tracepoint(_) | Program::Kprobe(_) | Program::Uprobe(_) => {
                self.add_single_attach_program(&mut program)
            }
            Program::Unsupported(_) => panic!("Cannot add unsupported program"),
        };

        // Program bytes MUST be cleared after load.
        program.get_data_mut().clear_program_bytes();

        match result {
            Ok(id) => {
                info!(
                    "Added {} program with name: {} and id: {id}",
                    program.kind(),
                    program.get_data().get_name()?
                );

                // Now that program is successfully loaded, update the id, maps hash table,
                // and allow access to all maps by bpfman group members.
                self.save_map(&mut program, id, map_owner_id)?;

                // Swap the db tree to be persisted with the unique program ID generated
                // by the kernel.
                program.get_data_mut().swap_tree(id)?;

                // Only add program to bpfManager if we've completed all mutations and it's successfully loaded.
                self.programs.insert(id, program.to_owned());

                Ok(program)
            }
            Err(e) => {
                // Cleanup any directories associated with the map_pin_path.
                // map_pin_path may or may not exist depending on where the original
                // error occured, so don't error if not there and preserve original error.
                if let Some(pin_path) = program.get_data().get_map_pin_path()? {
                    let _ = self.cleanup_map_pin_path(&pin_path, map_owner_id);
                }
                Err(e)
            }
        }
    }

    pub(crate) fn add_multi_attach_program(
        &mut self,
        program: &mut Program,
    ) -> Result<u32, BpfmanError> {
        debug!("BpfManager::add_multi_attach_program()");
        let name = &program.get_data().get_name()?;
        let allow_unsigned = self.allow_unsigned();
        // This load is just to verify the BPF Function Name is valid.
        // The actual load is performed in the XDP or TC logic.
        // don't pin maps here.
        let mut ext_loader = BpfLoader::new()
            .allow_unsupported_maps()
            .extension(name)
            .load(program.get_data().program_bytes())?;

        match ext_loader.program_mut(name) {
            Some(_) => Ok(()),
            None => Err(BpfmanError::BpfFunctionNameNotValid(name.to_owned())),
        }?;

        let did = program
            .dispatcher_id()?
            .ok_or(BpfmanError::DispatcherNotRequired)?;

        let next_available_id = self.dispatchers.attached_programs(&did);
        if next_available_id >= 10 {
            return Err(BpfmanError::TooManyPrograms);
        }

        debug!("next_available_id={next_available_id}");

        let program_type = program.kind();
        let if_index = program.if_index()?;
        let if_name = program.if_name().unwrap().to_string();
        let direction = program.direction()?;

        self.programs.add_and_set_program_positions(program);

        let mut programs: Vec<&mut Program> = self
            .programs
            .programs_mut(&program_type, &if_index, &direction)
            .collect::<Vec<&mut Program>>();

        // add the program that's being loaded
        programs.push(program);

        let old_dispatcher = self.dispatchers.remove(&did);
        let if_config = if let Some(ref i) = self.config.interfaces {
            i.get(&if_name)
        } else {
            None
        };
        let next_revision = if let Some(ref old) = old_dispatcher {
            old.next_revision()
        } else {
            1
        };

        let dispatcher = Dispatcher::new(
            if_config,
            &mut programs,
            next_revision,
            old_dispatcher,
            allow_unsigned,
        )
        .or_else(|e| {
            // If kernel ID was never set there's no pins to cleanup here so just continue
            if program.get_data().get_id().is_ok() {
                program
                    .delete()
                    .map_err(BpfmanError::BpfmanProgramDeleteError)?;
            }
            Err(e)
        })?;

        self.dispatchers.insert(did, dispatcher);
        let id = program.get_data().get_id()?;
        program.set_attached();

        Ok(id)
    }

    pub(crate) fn add_single_attach_program(
        &mut self,
        p: &mut Program,
    ) -> Result<u32, BpfmanError> {
        debug!("BpfManager::add_single_attach_program()");
        let name = &p.get_data().get_name()?;
        let mut bpf = BpfLoader::new();

        let data = &p.get_data().get_global_data()?;
        for (key, value) in data {
            bpf.set_global(key, value.as_slice(), true);
        }

        // If map_pin_path is set already it means we need to use a pin
        // path which should already exist on the system.
        if let Some(map_pin_path) = p.get_data().get_map_pin_path()? {
            debug!(
                "single-attach program {name} is using maps from {:?}",
                map_pin_path
            );
            bpf.map_pin_path(map_pin_path);
        }

        let mut loader = bpf
            .allow_unsupported_maps()
            .load(p.get_data().program_bytes())?;

        let raw_program = loader
            .program_mut(name)
            .ok_or(BpfmanError::BpfFunctionNameNotValid(name.to_owned()))?;

        let res = match p {
            Program::Tracepoint(ref mut program) => {
                let tracepoint = program.get_tracepoint()?;
                let parts: Vec<&str> = tracepoint.split('/').collect();
                if parts.len() != 2 {
                    return Err(BpfmanError::InvalidAttach(
                        program.get_tracepoint()?.to_string(),
                    ));
                }
                let category = parts[0].to_owned();
                let name = parts[1].to_owned();

                let tracepoint: &mut TracePoint = raw_program.try_into()?;

                tracepoint.load()?;
                program
                    .get_data_mut()
                    .set_kernel_info(&tracepoint.info()?)?;

                let id = program.data.get_id()?;

                let link_id = tracepoint.attach(&category, &name)?;

                let owned_link: TracePointLink = tracepoint.take_link(link_id)?;
                let fd_link: FdLink = owned_link
                    .try_into()
                    .expect("unable to get owned tracepoint attach link");

                fd_link
                    .pin(format!("{RTDIR_FS}/prog_{}_link", id))
                    .map_err(BpfmanError::UnableToPinLink)?;

                tracepoint
                    .pin(format!("{RTDIR_FS}/prog_{id}"))
                    .map_err(BpfmanError::UnableToPinProgram)?;

                Ok(id)
            }
            Program::Kprobe(ref mut program) => {
                let requested_probe_type = match program.get_retprobe()? {
                    true => Kretprobe,
                    false => Kprobe,
                };

                if requested_probe_type == Kretprobe && program.get_offset()? != 0 {
                    return Err(BpfmanError::Error(format!(
                        "offset not allowed for {Kretprobe}"
                    )));
                }

                let kprobe: &mut KProbe = raw_program.try_into()?;
                kprobe.load()?;

                // verify that the program loaded was the same type as the
                // user requested
                let loaded_probe_type = ProbeType::from(kprobe.kind());
                if requested_probe_type != loaded_probe_type {
                    return Err(BpfmanError::Error(format!(
                        "expected {requested_probe_type}, loaded program is {loaded_probe_type}"
                    )));
                }

                program.get_data_mut().set_kernel_info(&kprobe.info()?)?;

                let id = program.data.get_id()?;

                let link_id = kprobe.attach(program.get_fn_name()?, program.get_offset()?)?;

                let owned_link: KProbeLink = kprobe.take_link(link_id)?;
                let fd_link: FdLink = owned_link
                    .try_into()
                    .expect("unable to get owned kprobe attach link");

                fd_link
                    .pin(format!("{RTDIR_FS}/prog_{}_link", id))
                    .map_err(BpfmanError::UnableToPinLink)?;

                kprobe
                    .pin(format!("{RTDIR_FS}/prog_{id}"))
                    .map_err(BpfmanError::UnableToPinProgram)?;

                Ok(id)
            }
            Program::Uprobe(ref mut program) => {
                let requested_probe_type = match program.get_retprobe()? {
                    true => Uretprobe,
                    false => Uprobe,
                };

                let uprobe: &mut UProbe = raw_program.try_into()?;
                uprobe.load()?;

                // verify that the program loaded was the same type as the
                // user requested
                let loaded_probe_type = ProbeType::from(uprobe.kind());
                if requested_probe_type != loaded_probe_type {
                    return Err(BpfmanError::Error(format!(
                        "expected {requested_probe_type}, loaded program is {loaded_probe_type}"
                    )));
                }

                program.get_data_mut().set_kernel_info(&uprobe.info()?)?;

                let id = program.data.get_id()?;

                let program_pin_path = format!("{RTDIR_FS}/prog_{id}");
                let fn_name = program.get_fn_name()?;

                uprobe
                    .pin(program_pin_path.clone())
                    .map_err(BpfmanError::UnableToPinProgram)?;

                match program.get_container_pid()? {
                    None => {
                        // Attach uprobe in same container as the bpfman process
                        let link_id = uprobe.attach(
                            fn_name.as_deref(),
                            program.get_offset()?,
                            program.get_target()?,
                            None,
                        )?;

                        let owned_link: UProbeLink = uprobe.take_link(link_id)?;
                        let fd_link: FdLink = owned_link
                            .try_into()
                            .expect("unable to get owned uprobe attach link");

                        fd_link
                            .pin(format!("{RTDIR_FS}/prog_{}_link", id))
                            .map_err(BpfmanError::UnableToPinLink)?;
                    }
                    Some(p) => {
                        // Attach uprobe in different container from the bpfman process
                        let offset = program.get_offset()?.to_string();
                        let container_pid = p.to_string();
                        let mut prog_args = vec![
                            "uprobe".to_string(),
                            "--program-pin-path".to_string(),
                            program_pin_path,
                            "--offset".to_string(),
                            offset,
                            "--target".to_string(),
                            program.get_target()?.to_string(),
                            "--container-pid".to_string(),
                            container_pid,
                        ];

                        if let Some(fn_name) = &program.get_fn_name()? {
                            prog_args.extend(["--fn-name".to_string(), fn_name.to_string()])
                        }

                        if program.get_retprobe()? {
                            prog_args.push("--retprobe".to_string());
                        }

                        if let Some(pid) = program.get_pid()? {
                            prog_args.extend(["--pid".to_string(), pid.to_string()])
                        }

                        let status = std::process::Command::new("./target/debug/bpfman-ns")
                            .args(prog_args)
                            .status()
                            .expect("bpfman-ns call failed to return status");

                        debug!("bpfman-ns status: {:?}", status);

                        if !status.success() {
                            return Err(BpfmanError::ContainerAttachError {
                                program_type: "uprobe".to_string(),
                                container_pid: program.get_container_pid()?.unwrap(),
                            });
                        }
                    }
                };

                Ok(id)
            }
            _ => panic!("not a supported single attach program"),
        };

        match res {
            Ok(id) => {
                // If this program is the map(s) owner pin all maps (except for .rodata and .bss) by name.
                if p.get_data().get_map_pin_path()?.is_none() {
                    let map_pin_path = calc_map_pin_path(id);
                    p.get_data_mut().set_map_pin_path(&map_pin_path)?;
                    create_map_pin_path(&map_pin_path)?;

                    for (name, map) in loader.maps_mut() {
                        if !should_map_be_pinned(name) {
                            continue;
                        }
                        debug!(
                            "Pinning map: {name} to path: {}",
                            map_pin_path.join(name).display()
                        );
                        map.pin(map_pin_path.join(name))
                            .map_err(BpfmanError::UnableToPinMap)?;
                    }
                }
            }
            Err(_) => {
                // If kernel ID was never set there's no pins to cleanup here so just continue
                if p.get_data().get_id().is_ok() {
                    p.delete().map_err(BpfmanError::BpfmanProgramDeleteError)?;
                };
            }
        };

        res
    }

    pub(crate) fn remove_program(&mut self, id: u32) -> Result<(), BpfmanError> {
        info!("Removing program with id: {id}");
        let prog = match self.programs.remove(&id) {
            Some(p) => p,
            None => {
                return Err(BpfmanError::Error(format!(
                    "Program {0} does not exist or was not created by bpfman",
                    id,
                )));
            }
        };

        let map_owner_id = prog.get_data().get_map_owner_id()?;

        match prog {
            Program::Xdp(_) | Program::Tc(_) => self.remove_multi_attach_program(&prog)?,
            Program::Tracepoint(_)
            | Program::Kprobe(_)
            | Program::Uprobe(_)
            | Program::Unsupported(_) => (),
        }

        self.delete_map(id, map_owner_id)?;

        prog.delete()
            .map_err(BpfmanError::BpfmanProgramDeleteError)?;

        Ok(())
    }

    pub(crate) fn remove_multi_attach_program(
        &mut self,
        program: &Program,
    ) -> Result<(), BpfmanError> {
        debug!("BpfManager::remove_multi_attach_program()");
        let allow_unsigned = self.allow_unsigned();

        let did = program
            .dispatcher_id()?
            .ok_or(BpfmanError::DispatcherNotRequired)?;

        let next_available_id = self.dispatchers.attached_programs(&did) - 1;
        debug!("next_available_id = {next_available_id}");

        let mut old_dispatcher = self.dispatchers.remove(&did);

        if let Some(ref mut old) = old_dispatcher {
            if next_available_id == 0 {
                // Delete the dispatcher
                return old.delete(true);
            }
        }

        self.programs.set_program_positions(
            program.kind(),
            program.if_index()?.unwrap(),
            program.direction()?,
        );

        let program_type = program.kind();
        let if_index = program.if_index()?;
        let if_name = program.if_name().unwrap();
        let direction = program.direction()?;

        // Intentionally don't add filter program here
        let mut programs: Vec<&mut Program> = self
            .programs
            .programs_mut(&program_type, &if_index, &direction)
            .collect();

        let if_config = if let Some(ref i) = self.config.interfaces {
            i.get(&if_name)
        } else {
            None
        };
        let next_revision = if let Some(ref old) = old_dispatcher {
            old.next_revision()
        } else {
            1
        };
        debug!("next_revision = {next_revision}");
        let dispatcher = Dispatcher::new(
            if_config,
            &mut programs,
            next_revision,
            old_dispatcher,
            allow_unsigned,
        )?;
        self.dispatchers.insert(did, dispatcher);
        Ok(())
    }

    pub(crate) fn rebuild_multiattach_dispatcher(
        &mut self,
        did: DispatcherId,
        if_index: u32,
        program_type: ProgramType,
        direction: Option<Direction>,
    ) -> Result<(), BpfmanError> {
        debug!("BpfManager::rebuild_multiattach_dispatcher() for program type {program_type} on if_index {if_index:?}");
        let allow_unsigned = self.allow_unsigned();
        let mut old_dispatcher = self.dispatchers.remove(&did);

        if let Some(ref mut old) = old_dispatcher {
            debug!("Rebuild Multiattach Dispatcher for {did:?}");
            self.programs
                .set_program_positions(program_type, if_index, direction);
            let if_index = Some(if_index);
            let mut programs: Vec<&mut Program> = self
                .programs
                .programs_mut(&program_type, &if_index, &direction)
                .collect();

            debug!("programs loaded: {}", programs.len());

            // The following checks should have been done when the dispatcher was built, but check again to confirm
            if programs.is_empty() {
                return old.delete(true);
            } else if programs.len() > 10 {
                return Err(BpfmanError::TooManyPrograms);
            }

            let if_name = old.if_name();
            let if_config = if let Some(ref i) = self.config.interfaces {
                i.get(&if_name)
            } else {
                None
            };

            let next_revision = if let Some(ref old) = old_dispatcher {
                old.next_revision()
            } else {
                1
            };

            let dispatcher = Dispatcher::new(
                if_config,
                &mut programs,
                next_revision,
                old_dispatcher,
                allow_unsigned,
            )?;
            self.dispatchers.insert(did, dispatcher);
        } else {
            debug!("No dispatcher found in rebuild_multiattach_dispatcher() for {did:?}");
        }
        Ok(())
    }

    pub(crate) fn list_programs(&mut self) -> Result<Vec<Program>, BpfmanError> {
        debug!("BpfManager::list_programs()");

        // Get an iterator for the bpfman load programs, a hash map indexed by program id.
        let mut bpfman_progs: HashMap<u32, &Program> = self.programs.get_programs_iter().collect();

        // Call Aya to get ALL the loaded eBPF programs, and loop through each one.
        loaded_programs()
            .map(|p| {
                let prog = p.map_err(BpfmanError::BpfProgramError)?;
                let prog_id = prog.id();

                // If the program was loaded by bpfman (check the hash map), then use it.
                // Otherwise, convert the data returned from Aya into an Unsupported Program Object.
                match bpfman_progs.remove(&prog_id) {
                    Some(p) => Ok(p.to_owned()),
                    None => {
                        let db_tree = ROOT_DB
                            .open_tree(prog_id.to_string())
                            .expect("Unable to open program database tree for listing programs");

                        let mut data = ProgramData::new(db_tree, prog_id);
                        data.set_kernel_info(&prog)?;

                        Ok(Program::Unsupported(data))
                    }
                }
            })
            .collect()
    }

    pub(crate) fn get_program(&mut self, id: u32) -> Result<Program, BpfmanError> {
        debug!("Getting program with id: {id}");
        // If the program was loaded by bpfman, then use it.
        // Otherwise, call Aya to get ALL the loaded eBPF programs, and convert the data
        // returned from Aya into an Unsupported Program Object.
        match self.programs.get(&id) {
            Some(p) => Ok(p.to_owned()),
            None => loaded_programs()
                .find_map(|p| {
                    let prog = p.ok()?;
                    if prog.id() == id {
                        let db_tree = ROOT_DB
                            .open_tree(prog.id().to_string())
                            .expect("Unable to open program database tree for listing programs");

                        let mut data = ProgramData::new(db_tree, prog.id());
                        data.set_kernel_info(&prog)
                            .expect("unable to set kernel info");

                        Some(Program::Unsupported(data))
                    } else {
                        None
                    }
                })
                .ok_or(BpfmanError::Error(format!(
                    "Program {0} does not exist",
                    id
                ))),
        }
    }

    fn pull_bytecode(&self, args: PullBytecodeArgs) -> anyhow::Result<()> {
        let mut image_manager = IMAGE_MANAGER.lock().unwrap();
        let pull_result: Result<(String, String), crate::oci_utils::ImageError> = image_manager
            .pull(
                &args.image.image_url,
                args.image.image_pull_policy.clone(),
                args.image.username.clone(),
                args.image.password.clone(),
                self.allow_unsigned(),
            );
        let res = match pull_result {
            Ok(_) => {
                info!("Successfully pulled bytecode");
                Ok(())
            }
            Err(e) => Err(BpfmanError::BpfBytecodeError(e)),
        };
        let _ = args.responder.send(res);
        Ok(())
    }

    pub(crate) async fn process_commands(&mut self, use_activity_timer: bool) {
        loop {
            // Start receiving messages
            select! {
                biased;
                _ = shutdown_handler(use_activity_timer) => {
                    info!("Signal received to stop command processing");
                    ROOT_DB.flush().expect("Unable to flush database to disk before shutting down BpfManager");
                    break;
                }
                Some(cmd) = self.commands.recv() => {
                    match cmd {
                        Command::Load(args) => {
                            let prog = self.add_program(args.program);
                            // Ignore errors as they'll be propagated to caller in the RPC status
                            let _ = args.responder.send(prog);
                        },
                        Command::Unload(args) => {
                            let res = self.remove_program(args.id);
                            // Ignore errors as they'll be propagated to caller in the RPC status
                            let _ = args.responder.send(res);
                        },
                        Command::List { responder } => {
                            let progs = self.list_programs();
                            // Ignore errors as they'll be propagated to caller in the RPC status
                            let _ = responder.send(progs);
                        }
                        Command::Get(args) => {
                            let prog = self.get_program(args.id);
                            // Ignore errors as they'll be propagated to caller in the RPC status
                            let _ = args.responder.send(prog);
                        },
                        Command::PullBytecode (args) => self.pull_bytecode(args).unwrap(),
                    }
                }
            }
        }
        info!("Stopping processing commands");
    }

    // This function checks to see if the user provided map_owner_id is valid.
    fn is_map_owner_id_valid(&mut self, map_owner_id: u32) -> Result<PathBuf, BpfmanError> {
        let map_pin_path = calc_map_pin_path(map_owner_id);

        if self.maps.contains_key(&map_owner_id) {
            // Return the map_pin_path
            return Ok(map_pin_path);
        }
        Err(BpfmanError::Error(
            "map_owner_id does not exists".to_string(),
        ))
    }

    // This function is called if the program's map directory was created,
    // but the eBPF program failed to load. save_map() has not been called,
    // so self.maps has not been updated for this program.
    // If the user provided a ID of program to share a map with,
    // then map the directory is still in use and there is nothing to do.
    // Otherwise, the map directory was created so it must
    // deleted.
    fn cleanup_map_pin_path(
        &mut self,
        map_pin_path: &Path,
        map_owner_id: Option<u32>,
    ) -> Result<(), BpfmanError> {
        if map_owner_id.is_none() {
            let _ = remove_dir_all(map_pin_path)
                .map_err(|e| BpfmanError::Error(format!("can't delete map dir: {e}")));
            Ok(())
        } else {
            Ok(())
        }
    }

    // This function writes the map to the map hash table. If this eBPF
    // program is the map owner, then a new entry is add to the map hash
    // table and permissions on the directory are updated to grant bpfman
    // user group access to all the maps in the directory. If this eBPF
    // program is not the owner, then the eBPF program ID is added to
    // the Used-By array.

    // TODO this should probably be program.save_map not bpfmanager.save_map
    fn save_map(
        &mut self,
        program: &mut Program,
        id: u32,
        map_owner_id: Option<u32>,
    ) -> Result<(), BpfmanError> {
        let data = program.get_data_mut();

        match map_owner_id {
            Some(m) => {
                if let Some(map) = self.maps.get_mut(&m) {
                    map.used_by.push(id);

                    // This program has no been inserted yet, so set map_used_by to
                    // newly updated list.
                    data.set_maps_used_by(map.used_by.clone())?;

                    // Update all the programs using the same map with the updated map_used_by.
                    for used_by_id in map.used_by.iter() {
                        if let Some(program) = self.programs.get_mut(used_by_id) {
                            program
                                .get_data_mut()
                                .set_maps_used_by(map.used_by.clone())?;
                        }
                    }
                } else {
                    return Err(BpfmanError::Error(
                        "map_owner_id does not exists".to_string(),
                    ));
                }
            }
            None => {
                let map = BpfMap { used_by: vec![id] };

                self.maps.insert(id, map);

                // Update this program with the updated map_used_by
                data.set_maps_used_by(vec![id])?;

                // Set the permissions on the map_pin_path directory.
                if let Some(map_pin_path) = data.get_map_pin_path()? {
                    if let Some(path) = map_pin_path.to_str() {
                        debug!("bpf set dir permissions for {}", path);
                        set_dir_permissions(path, MAPS_MODE);
                    } else {
                        return Err(BpfmanError::Error(format!(
                            "invalid map_pin_path {} for {}",
                            map_pin_path.display(),
                            id
                        )));
                    }
                } else {
                    return Err(BpfmanError::Error(format!(
                        "map_pin_path should be set for {}",
                        id
                    )));
                }
            }
        }

        Ok(())
    }

    // This function cleans up a map entry when an eBPF program is
    // being unloaded. If the eBPF program is the map owner, then
    // the map is removed from the hash table and the associated
    // directory is removed. If this eBPF program is referencing a
    // map from another eBPF program, then this eBPF programs ID
    // is removed from the UsedBy array.
    fn delete_map(&mut self, id: u32, map_owner_id: Option<u32>) -> Result<(), BpfmanError> {
        let index = match map_owner_id {
            Some(i) => i,
            None => id,
        };

        if let Some(map) = self.maps.get_mut(&index.clone()) {
            if let Some(index) = map.used_by.iter().position(|value| *value == id) {
                map.used_by.swap_remove(index);
            }

            if map.used_by.is_empty() {
                // No more programs using this map, so remove the entry from the map list.
                let path = calc_map_pin_path(index);
                self.maps.remove(&index.clone());
                remove_dir_all(path)
                    .map_err(|e| BpfmanError::Error(format!("can't delete map dir: {e}")))?;
            } else {
                // Update all the programs still using the same map with the updated map_used_by.
                for id in map.used_by.iter() {
                    if let Some(program) = self.programs.get_mut(id) {
                        program
                            .get_data_mut()
                            .set_maps_used_by(map.used_by.clone())?;
                    }
                }
            }
        } else {
            return Err(BpfmanError::Error(
                "map_pin_path does not exists".to_string(),
            ));
        }

        Ok(())
    }

    fn rebuild_map_entry(&mut self, id: u32, program: &mut Program) {
        let map_owner_id = program.get_data().get_map_owner_id().unwrap();
        let index = match map_owner_id {
            Some(i) => i,
            None => id,
        };

        if let Some(map) = self.maps.get_mut(&index) {
            map.used_by.push(id);

            // This program has not been inserted yet, so update it with the
            // updated map_used_by.
            program
                .get_data_mut()
                .set_maps_used_by(map.used_by.clone())
                .expect("unable to set map_used_by");

            // Update all the other programs using the same map with the updated map_used_by.
            for used_by_id in map.used_by.iter() {
                // program may not exist yet on rebuild, so ignore if not there
                if let Some(prog) = self.programs.get_mut(used_by_id) {
                    prog.get_data_mut()
                        .set_maps_used_by(map.used_by.clone())
                        .unwrap();
                }
            }
        } else {
            let map = BpfMap { used_by: vec![id] };
            self.maps.insert(index, map);

            program.get_data_mut().set_maps_used_by(vec![id]).unwrap();
        }
    }
}

// map_pin_path is a the directory the maps are located. Currently, it
// is a fixed bpfman location containing the map_index, which is a ID.
// The ID is either the programs ID, or the ID of another program
// that map_owner_id references.
pub fn calc_map_pin_path(id: u32) -> PathBuf {
    PathBuf::from(format!("{RTDIR_FS_MAPS}/{}", id))
}

// Create the map_pin_path for a given program.
pub fn create_map_pin_path(p: &Path) -> Result<(), BpfmanError> {
    create_dir_all(p).map_err(|e| BpfmanError::Error(format!("can't create map dir: {e}")))
}
