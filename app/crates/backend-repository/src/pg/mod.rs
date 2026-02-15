use lru::LruCache;
use sqlx::PgPool;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
pub struct PgRepository {
    pub kyc: KycRepository,
    pub user: UserRepository,
    pub device: DeviceRepository,
    pub approval: ApprovalRepository,
    pub sms: SmsRepository,
}

impl PgRepository {
    pub fn new(pool: PgPool) -> Self {
        let capacity = NonZeroUsize::new(50_000).expect("non-zero LRU capacity");
        let resolve_user_by_phone_cache = Arc::new(Mutex::new(LruCache::new(capacity)));

        Self {
            kyc: KycRepository::new(pool.clone()),
            user: UserRepository::new(pool.clone(), resolve_user_by_phone_cache),
            device: DeviceRepository::new(pool.clone()),
            approval: ApprovalRepository::new(pool.clone()),
            sms: SmsRepository::new(pool),
        }
    }
}
