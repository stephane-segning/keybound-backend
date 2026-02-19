use anyhow::Result;
use backend_migrate::connect_postgres_and_migrate;
use backend_model::schema::app_user;
use backend_repository::{
    KycRepo, KycRepository, KycStepCreateInput, KycSubmissionFilter, MagicChallengeCreateInput,
    OtpChallengeCreateInput, UploadCompleteInput, UploadIntentCreateInput,
};
use chrono::{Duration, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::json;

#[tokio::test]
async fn kyc_repo_orchestration_flow() -> Result<()> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("Skipping backend-repository kyc test because DATABASE_URL is not set");
            return Ok(());
        }
    };

    let pool = connect_postgres_and_migrate(&database_url).await?;
    let repo = KycRepository::new(pool.clone());

    let user_id = backend_id::user_id()?;
    insert_test_user(&pool, &user_id, "kyc-flow-user", "Kyc", "Flow").await?;

    // session create/resume
    let (session1, step_ids1) = repo.start_or_resume_session(&user_id).await?;
    assert_eq!(session1.user_id, user_id);
    assert!(step_ids1.is_empty());

    let (session2, step_ids2) = repo.start_or_resume_session(&user_id).await?;
    assert_eq!(session1.id, session2.id);
    assert!(step_ids2.is_empty());

    // step create/get
    let phone_step = repo
        .create_step(KycStepCreateInput {
            session_id: session1.id.clone(),
            user_id: user_id.clone(),
            step_type: "PHONE".to_string(),
            policy: json!({"otp": true}),
        })
        .await?;
    let loaded_phone_step = repo.get_step(&phone_step.id).await?;
    assert!(loaded_phone_step.is_some());

    let email_step = repo
        .create_step(KycStepCreateInput {
            session_id: session1.id.clone(),
            user_id: user_id.clone(),
            step_type: "EMAIL".to_string(),
            policy: json!({"magic": true}),
        })
        .await?;

    // OTP issue/verify primitive coverage (success/invalid/expired/locked/rate-limited signals)
    let otp_ok = repo
        .create_otp_challenge(OtpChallengeCreateInput {
            step_id: phone_step.id.clone(),
            msisdn: "+10000000000".to_string(),
            channel: "SMS".to_string(),
            otp_hash: "argon2-ok".to_string(),
            expires_at: Utc::now() + Duration::minutes(5),
            tries_left: 3,
        })
        .await?;
    let fetched_otp_ok = repo
        .get_otp_challenge(&phone_step.id, &otp_ok.otp_ref)
        .await?
        .expect("otp challenge must exist");
    assert_eq!(fetched_otp_ok.otp_ref, otp_ok.otp_ref);

    repo.mark_otp_verified(&phone_step.id, &otp_ok.otp_ref)
        .await?;
    let verified_otp = repo
        .get_otp_challenge(&phone_step.id, &otp_ok.otp_ref)
        .await?
        .expect("otp challenge must exist after verification");
    assert!(verified_otp.verified_at.is_some());

    let otp_invalid = repo
        .create_otp_challenge(OtpChallengeCreateInput {
            step_id: phone_step.id.clone(),
            msisdn: "+10000000000".to_string(),
            channel: "SMS".to_string(),
            otp_hash: "argon2-invalid".to_string(),
            expires_at: Utc::now() + Duration::minutes(5),
            tries_left: 2,
        })
        .await?;
    let remaining_after_invalid = repo
        .decrement_otp_tries(&phone_step.id, &otp_invalid.otp_ref)
        .await?;
    assert_eq!(remaining_after_invalid, 1);

    let otp_locked = repo
        .create_otp_challenge(OtpChallengeCreateInput {
            step_id: phone_step.id.clone(),
            msisdn: "+10000000000".to_string(),
            channel: "SMS".to_string(),
            otp_hash: "argon2-locked".to_string(),
            expires_at: Utc::now() + Duration::minutes(5),
            tries_left: 1,
        })
        .await?;
    let remaining_after_lock = repo
        .decrement_otp_tries(&phone_step.id, &otp_locked.otp_ref)
        .await?;
    assert_eq!(remaining_after_lock, 0);

    let otp_expired = repo
        .create_otp_challenge(OtpChallengeCreateInput {
            step_id: phone_step.id.clone(),
            msisdn: "+10000000000".to_string(),
            channel: "SMS".to_string(),
            otp_hash: "argon2-expired".to_string(),
            expires_at: Utc::now() - Duration::minutes(1),
            tries_left: 3,
        })
        .await?;
    let fetched_expired = repo
        .get_otp_challenge(&phone_step.id, &otp_expired.otp_ref)
        .await?
        .expect("expired otp should still be retrievable");
    assert!(fetched_expired.expires_at < Utc::now());

    let otp_recent_count = repo
        .count_recent_otp_challenges(&phone_step.id, Utc::now() - Duration::minutes(10))
        .await?;
    assert!(otp_recent_count >= 3);

    // Magic issue/verify primitive coverage (success/invalid/expired/rate-limited signals)
    let magic_ok = repo
        .create_magic_challenge(MagicChallengeCreateInput {
            step_id: email_step.id.clone(),
            email: "kyc@example.com".to_string(),
            token_hash: "argon2-magic-ok".to_string(),
            expires_at: Utc::now() + Duration::minutes(10),
        })
        .await?;
    let fetched_magic = repo
        .get_magic_challenge(&magic_ok.token_ref)
        .await?
        .expect("magic challenge must exist");
    assert_eq!(fetched_magic.token_ref, magic_ok.token_ref);

    repo.mark_magic_verified(&magic_ok.token_ref).await?;
    let verified_magic = repo
        .get_magic_challenge(&magic_ok.token_ref)
        .await?
        .expect("magic challenge must exist after verification");
    assert!(verified_magic.verified_at.is_some());

    let magic_expired = repo
        .create_magic_challenge(MagicChallengeCreateInput {
            step_id: email_step.id.clone(),
            email: "kyc@example.com".to_string(),
            token_hash: "argon2-magic-expired".to_string(),
            expires_at: Utc::now() - Duration::minutes(1),
        })
        .await?;
    let fetched_magic_expired = repo
        .get_magic_challenge(&magic_expired.token_ref)
        .await?
        .expect("expired magic challenge should still be retrievable");
    assert!(fetched_magic_expired.expires_at < Utc::now());

    let magic_recent_count = repo
        .count_recent_magic_challenges(&email_step.id, Utc::now() - Duration::minutes(10))
        .await?;
    assert!(magic_recent_count >= 1);

    // Identity submission + upload completion idempotency + queue creation only after all 4 assets
    let identity_step = repo
        .create_step(KycStepCreateInput {
            session_id: session1.id.clone(),
            user_id: user_id.clone(),
            step_type: "IDENTITY".to_string(),
            policy: json!({"assets": 4}),
        })
        .await?;

    let required_assets = ["SELFIE_CLOSEUP", "SELFIE_WITH_ID", "ID_FRONT", "ID_BACK"];
    for (index, asset_type) in required_assets.iter().enumerate() {
        let upload = repo
            .create_upload_intent(UploadIntentCreateInput {
                step_id: identity_step.id.clone(),
                user_id: user_id.clone(),
                purpose: "KYC_IDENTITY".to_string(),
                asset_type: (*asset_type).to_string(),
                mime: "image/jpeg".to_string(),
                size_bytes: 1024,
                bucket: "kyc-bucket".to_string(),
                object_key: format!("{}/{}/{}.jpg", user_id, identity_step.id, asset_type),
                method: "PUT".to_string(),
                url: format!("https://example.com/{asset_type}"),
                headers: json!({"Content-Type": "image/jpeg"}),
                multipart: None,
                expires_at: Utc::now() + Duration::minutes(10),
            })
            .await?;

        let first_complete = repo
            .complete_upload_and_register_evidence(UploadCompleteInput {
                upload_id: upload.upload_id.clone(),
                user_id: user_id.clone(),
                bucket: upload.bucket.clone(),
                object_key: upload.object_key.clone(),
                etag: Some(format!("etag-{index}")),
                computed_sha256: Some(format!("sha-{index}")),
            })
            .await?;

        if index < 3 {
            assert!(!first_complete.moved_to_pending_review);
        }

        let second_complete = repo
            .complete_upload_and_register_evidence(UploadCompleteInput {
                upload_id: upload.upload_id,
                user_id: user_id.clone(),
                bucket: upload.bucket,
                object_key: upload.object_key,
                etag: Some(format!("etag-{index}")),
                computed_sha256: Some(format!("sha-{index}")),
            })
            .await?;

        assert_eq!(
            first_complete.evidence.evidence_id,
            second_complete.evidence.evidence_id
        );
    }

    let identity_after_uploads = repo
        .get_step(&identity_step.id)
        .await?
        .expect("identity step should exist");
    assert_eq!(identity_after_uploads.status, "PENDING_REVIEW");
    assert!(identity_after_uploads.submitted_at.is_some());

    // Staff submission list/detail + paging
    let (submission_rows, total_submissions) = repo
        .list_staff_submissions(KycSubmissionFilter {
            status: Some("PENDING_REVIEW".to_string()),
            search: Some("Kyc".to_string()),
            page: 1,
            limit: 10,
        })
        .await?;
    assert!(total_submissions >= 1);
    assert!(
        submission_rows
            .iter()
            .any(|row| row.submission_id == identity_step.id)
    );

    let detail = repo
        .get_staff_submission(&identity_step.id)
        .await?
        .expect("submission detail must exist");
    assert_eq!(detail.submission_id, identity_step.id);

    let docs = repo
        .list_staff_submission_documents(&identity_step.id)
        .await?;
    assert_eq!(docs.len(), 4);
    let doc_lookup = repo
        .get_staff_submission_document(&identity_step.id, &docs[0].id)
        .await?;
    assert!(doc_lookup.is_some());

    // Approve transition
    let approved = repo
        .approve_submission(&identity_step.id, Some("staff-1"), Some("looks good"))
        .await?;
    assert!(approved);
    let approved_step = repo
        .get_step(&identity_step.id)
        .await?
        .expect("approved step should exist");
    assert_eq!(approved_step.status, "VERIFIED");

    // Reject transition
    let reject_step = repo
        .create_step(KycStepCreateInput {
            session_id: session1.id.clone(),
            user_id: user_id.clone(),
            step_type: "IDENTITY".to_string(),
            policy: json!({}),
        })
        .await?;
    let rejected = repo
        .reject_submission(
            &reject_step.id,
            Some("staff-2"),
            "document mismatch",
            Some("retry"),
        )
        .await?;
    assert!(rejected);
    let rejected_step = repo
        .get_step(&reject_step.id)
        .await?
        .expect("rejected step should exist");
    assert_eq!(rejected_step.status, "REJECTED");

    // Request-info transition
    let request_info_step = repo
        .create_step(KycStepCreateInput {
            session_id: session1.id.clone(),
            user_id: user_id.clone(),
            step_type: "IDENTITY".to_string(),
            policy: json!({}),
        })
        .await?;
    let requested = repo
        .request_submission_info(&request_info_step.id, "Please re-upload a clear selfie")
        .await?;
    assert!(requested);
    let requested_step = repo
        .get_step(&request_info_step.id)
        .await?
        .expect("request-info step should exist");
    assert_eq!(requested_step.status, "IN_PROGRESS");

    // Review queue + case decision flow
    let review_step = repo
        .create_step(KycStepCreateInput {
            session_id: session1.id,
            user_id: user_id.clone(),
            step_type: "IDENTITY".to_string(),
            policy: json!({}),
        })
        .await?;
    register_identity_assets(&repo, &user_id, &review_step.id).await?;

    let (review_cases, pending_total_before) = repo.list_review_cases(1, 20).await?;
    assert!(pending_total_before >= 1);
    assert!(
        review_cases
            .iter()
            .any(|case| case.case_id == review_step.id)
    );

    let review_case = repo
        .get_review_case(&review_step.id)
        .await?
        .expect("review case should exist");
    assert_eq!(review_case.step_id, review_step.id);

    let decision = repo
        .decide_review_case(
            &review_step.id,
            "APPROVE",
            "OK",
            Some("approved in test"),
            Some("staff-3"),
        )
        .await?;
    assert!(decision.is_some());

    let (review_cases_after, _pending_total_after) = repo.list_review_cases(1, 20).await?;
    assert!(
        !review_cases_after
            .iter()
            .any(|case| case.case_id == review_step.id)
    );

    Ok(())
}

async fn register_identity_assets(
    repo: &KycRepository,
    user_id: &str,
    step_id: &str,
) -> Result<()> {
    let required_assets = ["SELFIE_CLOSEUP", "SELFIE_WITH_ID", "ID_FRONT", "ID_BACK"];

    for (index, asset_type) in required_assets.iter().enumerate() {
        let upload = repo
            .create_upload_intent(UploadIntentCreateInput {
                step_id: step_id.to_string(),
                user_id: user_id.to_string(),
                purpose: "KYC_IDENTITY".to_string(),
                asset_type: (*asset_type).to_string(),
                mime: "image/jpeg".to_string(),
                size_bytes: 1024,
                bucket: "kyc-bucket".to_string(),
                object_key: format!("{}/{}/{}.jpg", user_id, step_id, asset_type),
                method: "PUT".to_string(),
                url: format!("https://example.com/{asset_type}"),
                headers: json!({"Content-Type": "image/jpeg"}),
                multipart: None,
                expires_at: Utc::now() + Duration::minutes(10),
            })
            .await?;

        repo.complete_upload_and_register_evidence(UploadCompleteInput {
            upload_id: upload.upload_id,
            user_id: user_id.to_string(),
            bucket: upload.bucket,
            object_key: upload.object_key,
            etag: Some(format!("etag-r-{index}")),
            computed_sha256: Some(format!("sha-r-{index}")),
        })
        .await?;
    }

    Ok(())
}

async fn insert_test_user(
    pool: &diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>,
    user_id: &str,
    username: &str,
    first_name: &str,
    last_name: &str,
) -> Result<()> {
    let mut conn = pool.get().await?;
    diesel::insert_into(app_user::table)
        .values((
            app_user::user_id.eq(user_id),
            app_user::realm.eq("test"),
            app_user::username.eq(username),
            app_user::first_name.eq(Some(first_name.to_string())),
            app_user::last_name.eq(Some(last_name.to_string())),
            app_user::email.eq(Some(format!("{username}@example.com"))),
            app_user::phone_number.eq(Some("+10000000000".to_string())),
            app_user::disabled.eq(false),
            app_user::email_verified.eq(true),
            app_user::created_at.eq(Utc::now()),
            app_user::updated_at.eq(Utc::now()),
        ))
        .execute(&mut conn)
        .await?;

    Ok(())
}
