use serde::{Deserialize, Serialize};

pub const KIND_KYC_PHONE_OTP: &str = "KYC_PHONE_OTP";
pub const KIND_KYC_FIRST_DEPOSIT: &str = "KYC_FIRST_DEPOSIT";

pub const INSTANCE_STATUS_ACTIVE: &str = "ACTIVE";
pub const INSTANCE_STATUS_WAITING_INPUT: &str = "WAITING_INPUT";
pub const INSTANCE_STATUS_RUNNING: &str = "RUNNING";
pub const INSTANCE_STATUS_COMPLETED: &str = "COMPLETED";
pub const INSTANCE_STATUS_FAILED: &str = "FAILED";
pub const INSTANCE_STATUS_CANCELLED: &str = "CANCELLED";

pub const ATTEMPT_STATUS_QUEUED: &str = "QUEUED";
pub const ATTEMPT_STATUS_RUNNING: &str = "RUNNING";
pub const ATTEMPT_STATUS_SUCCEEDED: &str = "SUCCEEDED";
pub const ATTEMPT_STATUS_FAILED: &str = "FAILED";

pub const STEP_PHONE_ISSUE_OTP: &str = "ISSUE_OTP";
pub const STEP_PHONE_VERIFY_OTP: &str = "VERIFY_OTP";
pub const STEP_MARK_COMPLETE: &str = "MARK_COMPLETE";

pub const STEP_DEPOSIT_AWAIT_PAYMENT: &str = "AWAIT_PAYMENT_CONFIRMATION";
pub const STEP_DEPOSIT_AWAIT_APPROVAL: &str = "AWAIT_APPROVAL";
pub const STEP_DEPOSIT_REGISTER_CUSTOMER: &str = "REGISTER_CUSTOMER";
pub const STEP_DEPOSIT_APPROVE_AND_DEPOSIT: &str = "APPROVE_AND_DEPOSIT";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActorType {
    User,
    Staff,
    System,
}

impl ActorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActorType::User => "USER",
            ActorType::Staff => "STAFF",
            ActorType::System => "SYSTEM",
        }
    }
}
