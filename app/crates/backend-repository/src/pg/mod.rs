pub mod approval;
pub mod device;
pub mod kyc;
pub mod sms;
pub mod user;

pub use approval::{ApprovalRepository, PgApprovalRepo};
pub use device::{DeviceRepository, PgDeviceRepo};
pub use kyc::{KycRepository, PgKycRepo};
pub use sms::{PgSmsRepo, SmsRepository};
pub use user::{PgUserRepo, UserRepository};

