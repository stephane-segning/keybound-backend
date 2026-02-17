pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "kyc_case_status"))]
    pub struct KycCaseStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "kyc_submission_status"))]
    pub struct KycSubmissionStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "kyc_provisioning_status"))]
    pub struct KycProvisioningStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "kyc_document_status"))]
    pub struct KycDocumentStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "provisioning_status"))]
    pub struct ProvisioningStatus;
}

diesel::table! {
    app_user (user_id) {
        #[max_length = 40]
        user_id -> Varchar,
        #[max_length = 255]
        realm -> Varchar,
        #[max_length = 255]
        username -> Varchar,
        #[max_length = 255]
        first_name -> Nullable<Varchar>,
        #[max_length = 255]
        last_name -> Nullable<Varchar>,
        #[max_length = 320]
        email -> Nullable<Varchar>,
        email_verified -> Bool,
        #[max_length = 64]
        phone_number -> Nullable<Varchar>,
        #[max_length = 128]
        fineract_customer_id -> Nullable<Varchar>,
        disabled -> Bool,
        attributes -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::KycCaseStatus;

    kyc_case (id) {
        #[max_length = 40]
        id -> Varchar,
        #[max_length = 40]
        user_id -> Varchar,
        case_status -> Varchar,
        #[max_length = 40]
        active_submission_id -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::KycSubmissionStatus;
    use super::sql_types::KycProvisioningStatus;

    kyc_submission (id) {
        #[max_length = 40]
        id -> Varchar,
        #[max_length = 40]
        kyc_case_id -> Varchar,
        version -> Int4,
        status -> Varchar,
        submitted_at -> Nullable<Timestamptz>,
        decided_at -> Nullable<Timestamptz>,
        #[max_length = 40]
        decided_by -> Nullable<Varchar>,
        provisioning_status -> Varchar,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        #[max_length = 255]
        first_name -> Nullable<Varchar>,
        #[max_length = 255]
        last_name -> Nullable<Varchar>,
        #[max_length = 320]
        email -> Nullable<Varchar>,
        #[max_length = 64]
        phone_number -> Nullable<Varchar>,
        #[max_length = 64]
        date_of_birth -> Nullable<Varchar>,
        #[max_length = 128]
        nationality -> Nullable<Varchar>,
        rejection_reason -> Nullable<Text>,
        review_notes -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::KycDocumentStatus;

    kyc_document (id) {
        #[max_length = 40]
        id -> Varchar,
        #[max_length = 40]
        submission_id -> Varchar,
        #[max_length = 64]
        doc_type -> Varchar,
        #[max_length = 128]
        s3_bucket -> Varchar,
        #[max_length = 512]
        s3_key -> Varchar,
        #[max_length = 256]
        file_name -> Varchar,
        #[max_length = 128]
        mime_type -> Varchar,
        size_bytes -> Int8,
        #[max_length = 64]
        sha256 -> Bpchar,
        status -> Varchar,
        uploaded_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    fineract_provisioning (id) {
        #[max_length = 40]
        id -> Varchar,
        #[max_length = 40]
        kyc_case_id -> Varchar,
        #[max_length = 40]
        submission_id -> Varchar,
        status -> Varchar,
        #[max_length = 128]
        fineract_customer_id -> Nullable<Varchar>,
        #[max_length = 64]
        error_code -> Nullable<Varchar>,
        error_message -> Nullable<Text>,
        attempt_no -> Int4,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    device (device_id) {
        #[max_length = 40]
        device_id -> Varchar,
        #[max_length = 40]
        user_id -> Varchar,
        #[max_length = 255]
        jkt -> Varchar,
        public_jwk -> Text,
        #[max_length = 255]
        status -> Varchar,
        #[max_length = 255]
        label -> Nullable<Varchar>,
        created_at -> Timestamptz,
        last_seen_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    approval (request_id) {
        #[max_length = 40]
        request_id -> Varchar,
        #[max_length = 40]
        user_id -> Varchar,
        #[max_length = 40]
        new_device_id -> Varchar,
        #[max_length = 255]
        new_device_jkt -> Varchar,
        new_device_public_jwk -> Text,
        #[max_length = 64]
        new_device_platform -> Nullable<Varchar>,
        #[max_length = 128]
        new_device_model -> Nullable<Varchar>,
        #[max_length = 64]
        new_device_app_version -> Nullable<Varchar>,
        #[max_length = 255]
        status -> Varchar,
        created_at -> Timestamptz,
        expires_at -> Timestamptz,
        decided_at -> Nullable<Timestamptz>,
        #[max_length = 40]
        decided_by_device_id -> Nullable<Varchar>,
        #[max_length = 512]
        message -> Nullable<Varchar>,
    }
}

diesel::table! {
    sms_messages (id) {
        #[max_length = 40]
        id -> Varchar,
        #[max_length = 255]
        realm -> Varchar,
        #[max_length = 255]
        client_id -> Varchar,
        #[max_length = 40]
        user_id -> Nullable<Varchar>,
        #[max_length = 64]
        phone_number -> Varchar,
        #[max_length = 64]
        hash -> Varchar,
        otp_sha256 -> Bytea,
        ttl_seconds -> Int4,
        #[max_length = 32]
        status -> Varchar,
        attempt_count -> Int4,
        max_attempts -> Int4,
        next_retry_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
        #[max_length = 255]
        sns_message_id -> Nullable<Varchar>,
        #[max_length = 255]
        session_id -> Nullable<Varchar>,
        #[max_length = 255]
        trace_id -> Nullable<Varchar>,
        metadata -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        sent_at -> Nullable<Timestamptz>,
        confirmed_at -> Nullable<Timestamptz>,
    }
}

diesel::joinable!(kyc_case -> app_user (user_id));
diesel::joinable!(device -> app_user (user_id));
diesel::joinable!(approval -> app_user (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    app_user,
    kyc_case,
    kyc_submission,
    kyc_document,
    fineract_provisioning,
    device,
    approval,
    sms_messages,
);
