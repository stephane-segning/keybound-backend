# Welcome to the Keybound Backend! 🎉

## Why?
We built this project to handle tokenization and user storage with a focus on security, reliability, and smooth user experiences! We wanted a robust system that can manage KYC processes, device bindings, and staff operations all in one place while keeping things super clean and maintainable. 🚀

## Actual
Currently, we have a shiny Rust-based backend powered by `axum` and `diesel-async`. It features:
- **Three distinct API surfaces**: BFF for our users, KC for Keycloak integrations, and Staff for our amazing admin team. 🛠️
- **Flexible State Machines**: Handling complex flows like Phone OTP verification and First Deposit KYC with ease.
- **Top-notch Security**: Signature verification for Keycloak and OIDC-based authentication.
- **Background Workers**: Ensuring that tasks like SMS delivery and state machine steps are processed reliably in the background.

## Constraints
- We strictly use prefix + CUID2 for all IDs (no UUIDs here! 🙅‍♂️).
- All database operations go through Diesel DSL for type safety.
- No manual edits in `app/gen/*`—OpenAPI is our source of truth!
- We keep the server config centralized in `backend-core`.

## Findings
We've found that using a generic state machine store (`sm_instance`, `sm_event`, etc.) makes it incredibly easy to add new business processes without cluttering our database schema. It also gives us amazing observability into what's happening with every single user request! 🔍

## How to?
Want to dive in? Check out our other docs:
- [Architecture Overview](./architecture/overview.md) 🏗️
- [State Machines & Flows](./architecture/state-machines.md) 🔄
- [Auth & Security](./architecture/auth.md) 🔐
- [Development Guide](./development/setup.md) 💻
- [Getting Started](./development/getting-started.md) 🚀

## Conclusion
We're super proud of this backend and hope you enjoy working with it as much as we do! If you have any questions, don't hesitate to ask. Happy coding! ✨
