use num_derive::FromPrimitive;
use solana_program::{decode_error::DecodeError, program_error::ProgramError};
use thiserror::Error;

/// Errors that may be returned by the Token vesting program.
#[derive(Clone, Debug, Eq, Error, FromPrimitive, PartialEq)]
pub enum BonfidaBotError {
    // Invalid instruction
    #[error("Invalid Instruction")]
    InvalidInstruction,
}

impl From<BonfidaBotError> for ProgramError {
    fn from(e: BonfidaBotError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl<T> DecodeError<T> for BonfidaBotError {
    fn type_of() -> &'static str {
        "BonfidaBotError"
    }
}
