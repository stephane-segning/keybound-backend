#[cfg(test)]
mod tests {
    use gen_oas_server_kc::models::EnrollmentBindRequest;
    use serde_json::json;

    #[test]
    fn test_date_deserialization() {
        // Case 1: Date with Z (Success)
        let json_z = json!({
            "realm": "test",
            "client_id": "test",
            "user_id": "u1",
            "device_id": "d1",
            "jkt": "j1",
            "public_jwk": {"k": "v"},
            "created_at": "2026-02-17T11:46:14.994508389Z"
        });
        let req: Result<EnrollmentBindRequest, _> = serde_json::from_value(json_z);
        assert!(req.is_ok(), "Should succeed with Z");

        // Case 2: Date without Z (Failure?)
        let json_no_z = json!({
            "realm": "test",
            "client_id": "test",
            "user_id": "u1",
            "device_id": "d1",
            "jkt": "j1",
            "public_jwk": {"k": "v"},
            "created_at": "2026-02-17T11:46:14.994508389"
        });
        let req: Result<EnrollmentBindRequest, _> = serde_json::from_value(json_no_z);
        assert!(req.is_err(), "Should fail without Z");
    }
}
