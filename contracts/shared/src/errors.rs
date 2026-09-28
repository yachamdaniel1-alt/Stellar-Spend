use soroban_sdk::contracterror;

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    Unauthorized = 1,
    InvalidAmount = 2,
    NotFound = 3,
    AlreadyProcessed = 4,
    Expired = 5,
    BelowThreshold = 6,
    Paused = 7,
    Reentrant = 8,
    Overflow = 9,
    ContractFault = 10,
    AlreadyInitialized = 11,
    NotInitialized = 12,
    MigrationRequired = 13,
    SchemaVersionUnsupported = 14,
    SchemaAlreadyCurrent = 15,
    InvalidInput = 16,
    ArithmeticOverflow = 17,
    InsufficientBalance = 18,
}

pub type SharedError = ContractError;
