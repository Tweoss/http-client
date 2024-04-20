use std::{
    convert::TryFrom,
    fmt::{Display, Write},
};

use anyhow::{bail, ensure, Context, Result};

const HANDLE_LENGTH: usize = 32;

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize, Clone)]
pub(crate) struct Task {
    pub(crate) handle: Handle,
    pub(crate) operation: Operation,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Operation {
    Eval = 0,
    Apply = 1,
}

impl TryFrom<u8> for Operation {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Eval,
            1 => Self::Apply,
            _ => bail!("invalid u8 {} for Operation", value),
        })
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
pub(crate) struct Handle {
    pub(crate) content: [u8; HANDLE_LENGTH],
}

impl Handle {
    /// Parses a handle in format 64 character hex string
    pub(crate) fn from_hex(mut input: &str) -> Result<Self> {
        ensure!(input.len() == 64, "handle must be 64 hex characters");
        let mut content = [0_u8; HANDLE_LENGTH];

        for byte in &mut content {
            let (value, remaining) = input.split_at(2);
            input = remaining;
            *byte = u8::from_str_radix(value, 16).context("handle contains non-hex characters")?;
        }
        Ok(Self { content })
    }

    /// Reconstructs the hex string version of a Handle
    pub(crate) fn to_hex(&self) -> String {
        self.content.iter().fold(String::new(), |mut s, i| {
            write!(&mut s, "{:02x}", *i).expect("Failed to write to string");
            s
        })
    }
}

impl Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Operation::Apply => "Apply",
            Operation::Eval => "Eval",
        })
    }
}
