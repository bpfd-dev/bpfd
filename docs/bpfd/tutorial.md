# Tutorial

This tutorial will show you how to use `bpfd`.
There are several ways to launch and interact with `bpfd` and `bpfctl`:

* **Privileged Mode** - Run `bpfd` in foreground or background and straight from build directory.
  `bpfd` user is not created so `sudo` is always required when executing `bpfctl` commands.
  See [Privileged Mode](#privileged-mode).
* **Systemd Service** - Run `bpfd` as a systemd service as the `bpfd` user.
  See [Systemd Service](#systemd-service).

## Privileged Mode

### Step 0: Prerequisites

This tutorial uses examples from the [xdp-tutorial](https://github.com/xdp-project/xdp-tutorial).
Use either pre-built container images from
[https://quay.io/organization/bpfd-bytecode](https://quay.io/organization/bpfd-bytecode) or
check out the [xdp-tutorial](https://github.com/xdp-project/xdp-tutorial) git repository and
compile the examples.
[EBPF Bytecode Image Specifications](./shipping-bytecode.md) describes how BPF bytecode is
packaged in container images.

### Step 1: Build `bpfd`

Perform the following steps to build `bpfd`.
If this is your first time using bpfd, follow the instructions in
[Setup and Building bpfd](./building-bpfd.md) to setup the prerequisites for building.

```console
cd $HOME/src/bpfd/
cargo xtask build-ebpf --libbpf-dir $HOME/src/libbpf
cargo build
```

### Step 2: Setup `bpfd` environment

`bpfd` uses mTLS for mutual authentication with clients.
In order to run and interact with `bpfd`, certificates must be created.
If no local certificate authority exists when `bpfd` is started, it will automatically be created.
For this tutorial, `bpfd` will create the certificate authority in `/etc/bpfd/certs/`.

### Step 3: Start `bpfd`

While learning and experimenting with `bpfd`, it may be useful to run `bpfd` in the foreground
(which requires a second terminal to run the `bpfctl` commands below).
For more details on how logging is handled in bpfd, see [Logging](#logging) below.

```console
sudo RUST_LOG=info ./target/debug/bpfd
```

Later, once familiar with bpfd, optionally run in the background instead:

```console
sudo ./target/debug/bpfd&
```

### Step 4: Load your first program

We will load the simple `xdp-pass` program, which permits all traffic to the attached interface,
`vethb2795c7` in this example.
The section in the object file that contains the program is "xdp".
Finally, we will use the priority of 100 (valid values are from 0 to 255).
Find a deeper dive into `bpfctl` syntax in [bpfctl](#bpfctl) below.

```console
sudo ./target/debug/bpfctl load --location image://quay.io/bpfd-bytecode/xdp_pass:latest xdp --iface vethb2795c7 --priority 100
92e3e14c-0400-4a20-be2d-f701af21873c
```

`bpfctl` returns a unique identifier (`92e3e14c-0400-4a20-be2d-f701af21873c` in this example) to the program that was loaded.
This may be used to detach the program later.
We can check the program was loaded using the following command:

```console
sudo ./target/debug/bpfctl list
 UUID                                  Type  Name  Location                                       Metadata
 92e3e14c-0400-4a20-be2d-f701af21873c  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 100, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }
```

From the output above you can see the program was loaded to position 0 on our interface and will be executed first.

### Step 5: Loading more programs

We will now load 2 more programs with different priorities to demonstrate how bpfd will ensure they are ordered correctly:

```console
sudo ./target/debug/bpfctl load --location image://quay.io/bpfd-bytecode/xdp_pass:latest xdp --iface vethb2795c7 --priority 50
1ccc1376-60e8-4dc5-9079-6c32748fa1c4
```

```console
sudo ./target/debug/bpfctl load --location image://quay.io/bpfd-bytecode/xdp_pass:latest xdp --iface vethb2795c7 --priority 200
6af7c28f-6a7f-46ee-bc98-2d92ed261369
```

Using `bpfctl list` we can see that the programs are correctly ordered.
The lowest priority program is executed first, while the highest is executed last.

```console
sudo ./target/debug/bpfctl list
 UUID                                  Type  Name  Location                                       Metadata
 1ccc1376-60e8-4dc5-9079-6c32748fa1c4  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 50, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }
 92e3e14c-0400-4a20-be2d-f701af21873c  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 100, "iface": "vethb2795c7", "position": 1, "proceed_on": ["pass", "dispatcher_return"] }
 6af7c28f-6a7f-46ee-bc98-2d92ed261369  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 200, "iface": "vethb2795c7", "position": 2, "proceed_on": ["pass", "dispatcher_return"] }
```

By default, the next program in the chain will only be executed if a given program returns
`pass` (see `proceed-on` field in the `bpfctl list` output above).
If the next program in the chain should be called even if a different value is returned,
then the program can be loaded with those additional return values using the `proceed-on`
parameter (see `bpfctl help load` for list of valid values):

```console
sudo ./target/debug/bpfctl load --location image://quay.io/bpfd-bytecode/xdp_pass:latest xdp --iface vethb2795c7 --priority 150 --proceed-on "pass" --proceed-on "dispatcher_return"
b2f19b7b-4c71-4338-873e-914bd8fa44ba
```

Which results in (see position 2):

```console
sudo ./target/debug/bpfctl list
 UUID                                  Type  Name  Location                                       Metadata
 1ccc1376-60e8-4dc5-9079-6c32748fa1c4  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 50, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }
 b2f19b7b-4c71-4338-873e-914bd8fa44ba  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 150, "iface": "vethb2795c7", "position": 2, "proceed_on": ["pass", "dispatcher_return"] }
 6af7c28f-6a7f-46ee-bc98-2d92ed261369  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 200, "iface": "vethb2795c7", "position": 3, "proceed_on": ["pass", "dispatcher_return"] }
 92e3e14c-0400-4a20-be2d-f701af21873c  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 100, "iface": "vethb2795c7", "position": 1, "proceed_on": ["pass", "dispatcher_return"] }
```

Note that the list of programs may not always be sorted in the order of execution.
The `position` indicates the order of execution, low to high.

### Step 6: Delete a program

Let's remove the program at position 1.

```console
sudo ./target/debug/bpfctl unload 92e3e14c-0400-4a20-be2d-f701af21873c
```

And we can verify that it has been removed and the other programs re-ordered:

```console
sudo ./target/debug/bpfctl list
 UUID                                  Type  Name  Location                                       Metadata
 1ccc1376-60e8-4dc5-9079-6c32748fa1c4  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 50, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }
 b2f19b7b-4c71-4338-873e-914bd8fa44ba  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 150, "iface": "vethb2795c7", "position": 1, "proceed_on": ["pass", "dispatcher_return"] }
 6af7c28f-6a7f-46ee-bc98-2d92ed261369  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 200, "iface": "vethb2795c7", "position": 2, "proceed_on": ["pass", "dispatcher_return"] }
```

When `bpfd` is stopped, all remaining programs will be unloaded automatically.

### Step 7: Clean-up

To unwind all the changes, stop `bpfd` and then run the following script:

```console
sudo ./scripts/setup.sh uninstall
```

**WARNING:** `setup.sh uninstall` cleans everything up, so `/etc/bpfd/programs.d/`
and `/run/bpfd/bytecode/` are deleted. Save any changes or files that were created if needed.

## Systemd Service

To run `bpfd` as a systemd service, the binaries will be placed in a well known location
(`/usr/sbin/.`) and a service configuration file will be added
(`/usr/lib/systemd/system/bpfd.service`).
When run as a systemd service, the set of linux capabilities are limited to only the needed set.
If permission errors are encountered, see [Linux Capabilities](./linux-capabilities.md)
for help debugging.

### Step 0-1

Same as above.

### Step 2: Setup `bpfd` environment

Run the following command to copy the `bpfd` and `bpfctl` binaries to `/usr/sbin/.` and set the user
and user group for each, and copy a default `bpfd.service` file to `/usr/lib/systemd/system/`.
This option will also start the systemd service `bpfd.service` by default:

```console
sudo ./scripts/setup.sh install
```

Then add usergroup `bpfd` to the desired user if not already run and logout/login to apply.
Programs run by users which are members of the `bpfd` user group are able to access the mTLS certificates
created by bpfd.
Therefore, these programs can make bpfd requests without requiring `sudo`.
For userspace programs accessing maps, the maps are owned by the `bpfd` user and `bpfd` user group.
Programs run by users which are members of the `bpfd` user group are able to access the maps files without
requiring  `sudo` (specifically CAP_DAC_SEARCH or CAP_DAC_OVERIDE).

```console
sudo usermod -a -G bpfd $USER
exit
<LOGIN>
```

> **_NOTE:_** Prior to **kernel 5.19**, all BPF sys calls required CAP_BPF, which are used to access maps shared
between the BFP program and the userspace program.
So userspace programs that are accessing maps and running on kernels older than 5.19 will require either `sudo`
or the CAP_BPF capability (`sudo /sbin/setcap cap_bpf=ep ./<USERSPACE-PROGRAM>`).


To update the configuration settings associated with running `bpfd` as a service, edit the
service configuration file:

```console
sudo vi /usr/lib/systemd/system/bpfd.service
sudo systemctl daemon-reload
```

If `bpfd` or `bpfctl` is rebuilt, the following command can be run to install the update binaries
without tearing down the users and regenerating the certifications.
The `bpfd` service will is automatically restarted.

```console
sudo ./scripts/setup.sh reinstall
```

### Step 3: Start `bpfd`

To manage `bpfd` as a systemd service, use `systemctl`. `sudo ./scripts/setup.sh install` will start the service,
but the service can be manually stopped and started:

```console
sudo systemctl stop bpfd.service
...
sudo systemctl start bpfd.service
```

### Step 4-6

Same as above except `sudo` can be dropped from all the `bpfctl` commands and `bpfctl` is now in $PATH:

```console
bpfctl load --location image://quay.io/bpfd-bytecode/xdp_pass:latest xdp --iface vethb2795c7 --priority 100
92e3e14c-0400-4a20-be2d-f701af21873c

bpfctl list
 UUID                                  Type  Name  Location                                       Metadata
 92e3e14c-0400-4a20-be2d-f701af21873c  xdp   xdp   image://quay.io/bpfd-bytecode/xdp_pass:latest  { "priority": 100, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }

bpfctl unload 92e3e14c-0400-4a20-be2d-f701af21873c
```

### Step 7: Clean-up

To unwind all the changes performed while running `bpfd` as a systemd service, run the following
script. This command cleans up everything, including stopping the `bpfd` service if it is still
running.

```console
sudo ./scripts/setup.sh uninstall
```

**WARNING:** `setup.sh uninstall` cleans everything up, so `/etc/bpfd/programs.d/`
and `/run/bpfd/bytecode/` are deleted. Save any changes or files that were created if needed.

# bpfctl

`bpfctl` is the command line tool for interacting with `bpfd`.
`bpfctl` allows the user to `load`, `unload` and `list` bpf programs.
Basic syntax:

```console
bpfctl --help
A client for working with bpfd

Usage: bpfctl <COMMAND>

Commands:
  load
  unload
  list
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help information
  -V, --version  Print version information
```

## bpfctl load

The `bpfctl load` command is used to load bpf programs.
Each program type (i.e. `<COMMAND>`) has it's own set of attributes specific to the program type,
and those attributes MUST come after the program type is entered.
There are a common set of attributes, and those MUST come before the program type is entered.

```console
bpfctl load --help
Usage: bpfctl load [OPTIONS] --location <LOCATION> <COMMAND>

Commands:
  xdp
  tc
  tracepoint
  help        Print this message or the help of the given subcommand(s)

Options:
  -s, --section-name <SECTION_NAME>  Required if "--from-image" is not present: Name of the ELF section from the object file [default: ]
  -l, --location <LOCATION>          Required: Location of Program Bytecode to load. Either Local file (file:///<path>) or bytecode image URL (image://<container image url>)
  -h, --help                         Print help
```

Example loading from local file:

```console
bpfctl load --location file:///usr/local/src/xdp-tutorial/basic01-xdp-pass/xdp_pass_kern.o --section-name "xdp" xdp --iface vethb2795c7 --priority 100
```

Example from image in remote repository (Note: `--section-name` is built into the image and is not required):

```console
bpfctl load --location image://quay.io/bpfd-bytecode/xdp_pass:latest xdp --iface vethb2795c7 --priority 100
```

Command specific help is also provided:

```console
bpfctl load xdp --help
Usage: bpfctl load --location <LOCATION> xdp [OPTIONS] --iface <IFACE> --priority <PRIORITY>

Options:
  -i, --iface <IFACE>               Required: Interface to load program on
      --priority <PRIORITY>         Required: Priority to run program in chain. Lower value runs first
      --proceed-on <PROCEED_ON>...  Optional: Proceed to call other programs in chain on this exit code. Multiple values supported by repeating the parameter. Possible values: [aborted, drop, pass, tx, redirect, dispatcher_return] Default values: pass and dispatcher_return
  -h, --help                        Print help
```

### bpfctl load Examples

Below are some examples of `bpfctl load` commands:

```console
bpfctl load --location file:///usr/local/src/xdp-tutorial/basic01-xdp-pass/xdp_pass_kern.o --section-name "xdp" xdp --iface vethb2795c7 --priority 35

bpfctl load --location file:///usr/local/src/net-ebpf-playground/.output/filter.bpf.o --section-name classifier tc --direction ingress --iface vethb2795c7 --priority 110

bpfctl load --location image://quay.io/bpfd-bytecode/tracepoint:latest tracepoint --tracepoint sched/sched_switch
```

## bpfctl list

The `bpfctl list` command lists all the loaded bpf programs:

```console
bpfctl list
 UUID                                  Type  Name         Location                                                             Metadata
 c22440a7-5511-4c59-9cad-50583d0dbb3f  tc-0  classifier   file:///usr/local/src/net-ebpf-playground/.output/filter.bpf.o       { "priority": 110, "iface": "vethb2795c7", "postiion": 0 }
 6dafc471-a05c-469e-a066-0d2fbba8f19d  xdp   xdp          file:///usr/local/src/xdp-tutorial/basic01-xdp-pass/xdp_pass_kern.o  { "priority": 35, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }
 e46e9de9-321b-4d91-b7c5-426f4357402d  tracepoint  hello  image://quay.io/bpfd-bytecode/tracepoint:latest                      { "tracepoint": sched/sched_switch }
```

## bpfctl unload

The `bpfctl unload` command takes the UUID from the load or list command as a parameter, and unloads the requested bpf program:

```console
bpfctl unload c22440a7-5511-4c59-9cad-50583d0dbb3f

bpfctl list
 UUID                                  Type  Name         Location                                                             Metadata
 6dafc471-a05c-469e-a066-0d2fbba8f19d  xdp   xdp          file:///usr/local/src/xdp-tutorial/basic01-xdp-pass/xdp_pass_kern.o  { "priority": 35, "iface": "vethb2795c7", "position": 0, "proceed_on": ["pass", "dispatcher_return"] }
 e46e9de9-321b-4d91-b7c5-426f4357402d  tracepoint  hello  image://quay.io/bpfd-bytecode/tracepoint:latest                      { "tracepoint": sched/sched_switch }
```

# Logging

## env_logger

`bpfd` and `bpfctl` use the [env_logger](https://docs.rs/env_logger) crate to log messages to the terminal.
By default, only `error` messages are logged, but that can be overwritten by setting
the `RUST_LOG` environment variable.
Valid values:

* `error`
* `warn`
* `info`
* `debug`
* `trace`

Example:

```console
$ sudo RUST_LOG=info /usr/local/bin/bpfd
[2022-08-08T20:29:31Z INFO  bpfd] Log using env_logger
[2022-08-08T20:29:31Z INFO  bpfd::server] Loading static programs from /etc/bpfd/programs.d
[2022-08-08T20:29:31Z INFO  bpfd::server::bpf] Map veth12fa8e3 to 13
[2022-08-08T20:29:31Z INFO  bpfd::server] Listening on [::1]:50051
[2022-08-08T20:29:31Z INFO  bpfd::server::bpf] Program added: 1 programs attached to veth12fa8e3
[2022-08-08T20:29:31Z INFO  bpfd::server] Loaded static program pass with UUID d9fd88df-d039-4e64-9f63-19f3e08915ce
```

`bpfctl` doesn't currently have any logs, but the infrastructure is in place if needed for future debugging.

## Systemd Service

If `bpfd` is running as a systemd service, then `bpfd` will log to journald.
As with env_logger, by default, only `error` messages are logged, but that can be
overwritten by setting the `RUST_LOG` environment variable.
`bpfctl` won't be run as a service, so it always uses env_logger.

Example:

```console
sudo vi /usr/lib/systemd/system/bpfd.service
[Unit]
Description=Run bpfd as a service
DefaultDependencies=no
After=network.target

[Service]
Environment="RUST_LOG=Info"    <==== Set Log Level Here
ExecStart=/usr/sbin/bpfd
MemoryAccounting=true
MemoryLow=infinity
MemoryMax=infinity
User=bpfd
Group=bpfd
AmbientCapabilities=CAP_BPF CAP_DAC_READ_SEARCH CAP_NET_ADMIN CAP_PERFMON CAP_SYS_ADMIN CAP_SYS_RESOURCE
CapabilityBoundingSet=CAP_BPF CAP_DAC_READ_SEARCH CAP_NET_ADMIN CAP_PERFMON CAP_SYS_ADMIN CAP_SYS_RESOURCE
```

Start the service:

```console
sudo systemctl start bpfd.service
```

Check the logs:

```console
$ sudo journalctl -f -u bpfd
Aug 08 16:25:04 ebpf03 systemd[1]: Started bpfd.service - Run bpfd as a service.
Aug 08 16:25:04 ebpf03 bpfd[180118]: Log using journald
Aug 08 16:25:04 ebpf03 bpfd[180118]: Loading static programs from /etc/bpfd/programs.d
Aug 08 16:25:04 ebpf03 bpfd[180118]: Map veth12fa8e3 to 13
Aug 08 16:25:04 ebpf03 bpfd[180118]: Listening on [::1]:50051
Aug 08 16:25:04 ebpf03 bpfd[180118]: Program added: 1 programs attached to veth12fa8e3
Aug 08 16:25:04 ebpf03 bpfd[180118]: Loaded static program pass with UUID a3ffa14a-786d-48ad-b0cd-a4802f0f10b6
```

Stop the service:

```console
sudo systemctl stop bpfd.service
```
