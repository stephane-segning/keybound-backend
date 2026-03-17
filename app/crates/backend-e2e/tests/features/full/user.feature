Feature: Flow User Endpoints
  Verify user reads and KYC level behavior on the v2 flow surface

  Background:
    Given the e2e test environment is initialized
    And I have a valid authentication token
    And the database fixtures are set up
    And the SMS sink is reset

  @serial
  Scenario: Get user returns the authenticated subject
    When I get the current user
    Then the response status is 200
    And the response contains the correct user ID

  @serial
  Scenario: Initial KYC level is NONE
    When I get the KYC level
    Then the response status is 200
    And the KYC level is "NONE"
    And phoneOtpVerified is false
    And firstDepositVerified is false

  @serial
  Scenario: Phone OTP verification updates KYC level
    Given I complete phone OTP verification
    When I get the KYC level
    Then the response status is 200
    And the KYC level contains "PHONE_OTP_VERIFIED"
    And phoneOtpVerified is true
