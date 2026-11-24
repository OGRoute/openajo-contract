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
    OpenCircleCapExceeded = 14,
    /// Contribution attempted before the circle's scheduled start time.
    NotStarted = 15,
    /// swap_positions called on a member who has already received or defaulted.
    CannotSwap = 16,
    /// set_fee called with fee_bps above MAX_FEE_BPS.
    FeeTooHigh = 17,
    BadOrder = 18,
}
