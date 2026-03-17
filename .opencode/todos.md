# Fix Empty Recipient Issue - Implementation Tasks

## Phase 1: Create Production Builtin Steps
- [ ] Create `app/crates/backend-server/src/flow_logic/builtin_steps.rs`
- [ ] Add `CheckUserExistsStep` with real DB lookup and recipient resolution
- [ ] Add `ValidateDepositStep` with proper validation
- [ ] Add `PersistDepositResultStep` for saving results

## Phase 2: Update FIRST_DEPOSIT Flow
- [ ] Update `flow_logic/mod.rs` to export builtin_steps module
- [ ] Update `flow_logic/first_deposit.rs` with complete step chain
- [ ] Update `flow_registry.rs` to register builtin step types

## Phase 3: Implement Recipient Resolution
- [ ] Add recipient resolution logic to `backend-core/src/config.rs`
- [ ] Add deposit recipient repository traits to `backend-repository/src/traits.rs`
- [ ] Implement deposit recipient upsert methods in flow repository
- [ ] Add recipient sync logic to `state.rs` startup

## Phase 4: Integration & Testing
- [ ] Run compilation checks
- [ ] Test with existing flow registration
- [ ] Verify recipient data is populated correctly