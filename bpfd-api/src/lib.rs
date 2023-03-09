// SPDX-License-Identifier: (MIT OR Apache-2.0)
// Copyright Authors of bpfd

pub mod config;
pub mod util;
#[path = "bpfd.v1.rs"]
#[rustfmt::skip]
#[allow(clippy::all)]
pub mod v1;

use thiserror::Error;
use url::ParseError as urlParseError;
use v1::{Direction, ProceedOn, ProgramType};

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("{program} is not a valid program type")]
    InvalidProgramType { program: String },
    #[error("{proceedon} is not a valid proceed-on value")]
    InvalidProceedOn { proceedon: String },
    #[error("not a valid direction")]
    InvalidDirection,
    #[error("Failed to Parse bytecode location: {0}")]
    BytecodeLocationParseFailure(#[source] urlParseError),
    #[error("Invalid bytecode location: {location}")]
    InvalidBytecodeLocation { location: String },
    #[error("Failed to pull bytecode from container registry: {0}")]
    BytecodePullFaiure(#[source] anyhow::Error),
    #[error("Bytecode image has section name: {image_sec_name} isn't equal to the provided section name {provided_sec_name}")]
    BytecodeMetaDataMismatch {
        image_sec_name: String,
        provided_sec_name: String,
    },
}

impl ToString for ProgramType {
    fn to_string(&self) -> String {
        match &self {
            ProgramType::Xdp => "xdp".to_owned(),
            ProgramType::Tc => "tc".to_owned(),
            ProgramType::Tracepoint => "tracepoint".to_owned(),
        }
    }
}

impl TryFrom<String> for ProgramType {
    type Error = ParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(match value.as_str() {
            "xdp" => ProgramType::Xdp,
            "tc" => ProgramType::Tc,
            "tracepoint" => ProgramType::Tracepoint,
            program => {
                return Err(ParseError::InvalidProgramType {
                    program: program.to_string(),
                })
            }
        })
    }
}

impl ToString for ProceedOn {
    fn to_string(&self) -> String {
        match &self {
            ProceedOn::Aborted => "aborted".to_owned(),
            ProceedOn::Drop => "drop".to_owned(),
            ProceedOn::Pass => "pass".to_owned(),
            ProceedOn::Tx => "tx".to_owned(),
            ProceedOn::Redirect => "redirect".to_owned(),
            ProceedOn::DispatcherReturn => "dispatcher_return".to_owned(),
        }
    }
}

impl TryFrom<String> for ProceedOn {
    type Error = ParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(match value.as_str() {
            "aborted" => ProceedOn::Aborted,
            "drop" => ProceedOn::Drop,
            "pass" => ProceedOn::Pass,
            "tx" => ProceedOn::Tx,
            "redirect" => ProceedOn::Redirect,
            "dispatcher_return" => ProceedOn::DispatcherReturn,
            proceedon => {
                return Err(ParseError::InvalidProceedOn {
                    proceedon: proceedon.to_string(),
                })
            }
        })
    }
}

impl TryFrom<u32> for ProceedOn {
    type Error = ParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => ProceedOn::Aborted,
            1 => ProceedOn::Drop,
            2 => ProceedOn::Pass,
            3 => ProceedOn::Tx,
            4 => ProceedOn::Redirect,
            31 => ProceedOn::DispatcherReturn,
            proceedon => {
                return Err(ParseError::InvalidProceedOn {
                    proceedon: proceedon.to_string(),
                })
            }
        })
    }
}

impl TryFrom<i32> for ProgramType {
    type Error = ParseError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => ProgramType::Xdp,
            1 => ProgramType::Tc,
            2 => ProgramType::Tracepoint,
            other => {
                return Err(ParseError::InvalidProgramType {
                    program: other.to_string(),
                })
            }
        })
    }
}

impl ToString for Direction {
    fn to_string(&self) -> String {
        match &self {
            Direction::None => "none".to_string(),
            Direction::Ingress => "ingress".to_string(),
            Direction::Egress => "egress".to_string(),
        }
    }
}
