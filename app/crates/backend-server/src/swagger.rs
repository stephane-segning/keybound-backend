use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api::{
    bff_flow::BffFlowOpenApi, bff_uploads::BffUploadsOpenApi, staff_flow::StaffFlowOpenApi,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "KYC Tokenization Backend API",
        version = "0.2.4",
        description = "KYC orchestration backend with signature auth, flows, and webhook integration"
    ),
    tags(
        (name = "users", description = "User profile endpoints"),
        (name = "sessions", description = "Session management endpoints"),
        (name = "flows", description = "Flow execution endpoints"),
        (name = "steps", description = "Step submission endpoints"),
    )
)]
pub struct ApiDoc;

pub fn swagger_ui() -> SwaggerUi {
    let bff_spec = BffFlowOpenApi::openapi();
    let uploads_spec = BffUploadsOpenApi::openapi();
    let staff_spec = StaffFlowOpenApi::openapi();

    SwaggerUi::new("/swagger-ui/")
        .url("/api-docs/bff/openapi.json", bff_spec)
        .url("/api-docs/uploads/openapi.json", uploads_spec)
        .url("/api-docs/staff/openapi.json", staff_spec)
        .url("/api-docs/core/openapi.json", ApiDoc::openapi())
}
