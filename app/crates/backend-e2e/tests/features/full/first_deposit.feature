Feature: First Deposit Flow
  Exercise the v2 `first_deposit` flow through the BFF and staff surfaces

  Background:
    Given the e2e test environment is initialized
    And I have a valid authentication token
    And the database fixtures are set up
    And the SMS sink is reset
    And the CUSS sink is reset

  @serial
  Scenario: Approved first deposit updates KYC level and metadata
    Given I complete phone OTP verification
    Then no error occurred
    Given I start a first deposit flow for 5000 XAF
    Then no error occurred
    Then the first deposit flow is waiting for admin review
    When I approve the pending first deposit admin step
    Then the response status is 200
    And no error occurred
    And the staff flow detail shows the completed deposit path
    And CUSS register and approve requests were recorded
    And the CUSS payloads match the first deposit flow
    And the first deposit metadata is persisted
    When I get the KYC level
    Then the response status is 200
    And the KYC level contains "PHONE_OTP_VERIFIED"
    And the KYC level contains "FIRST_DEPOSIT_VERIFIED"
    And firstDepositVerified is true

  @serial
  Scenario: Rejected first deposit closes the session without CUSS activity
    Given I start a first deposit flow for 7000 XAF
    Then no error occurred
    Then the first deposit flow is waiting for admin review
    When I reject the pending first deposit admin step
    Then the response status is 200
    And no error occurred
    And the staff flow detail shows the rejected deposit path
    And the reject path closes the session with reason REJECTED_BY_ADMIN
    And no CUSS request was recorded
    When I get the current user
    Then the response status is 200
    And firstDepositVerified is false
    And the first deposit metadata is not persisted

  @serial
  Scenario: CUSS register failure is marked retryable
    Given the CUSS register endpoint fails with 500 for 3 attempts
    And I start a first deposit flow for 5000 XAF
    Then no error occurred
    Then the first deposit flow is waiting for admin review
    When I approve the pending first deposit admin step expecting flow failure
    Then the response status is 200
    And no error occurred
    And the first deposit step cuss_register_customer is failed and retryable

  @serial
  Scenario: CUSS approve failure is marked retryable
    Given the CUSS approve endpoint fails with 500 for 3 attempts
    And I start a first deposit flow for 5000 XAF
    Then no error occurred
    Then the first deposit flow is waiting for admin review
    When I approve the pending first deposit admin step expecting flow failure
    Then the response status is 200
    And no error occurred
    And the first deposit step cuss_approve_and_deposit is failed and retryable
