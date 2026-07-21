use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotFound = 3,
    BadStatus = 4,
    AlreadyMember = 5,
    NotMember = 6,
    CircleFull = 7,
    AlreadyPaid = 8,
    Defaulted = 9,
    NotDue = 10,
    IsCreator = 11,
    BadParams = 12,
    NotCreator = 13,
}
