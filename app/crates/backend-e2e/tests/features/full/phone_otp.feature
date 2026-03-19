Feature: Phone OTP Flow
  Validate the v2 `phone_otp` flow end-to-end through the BFF surface

  Background:
    Given the e2e test environment is initialized
    And I have a valid authentication token
    And the database fixtures are set up
    And the SMS sink is reset

  @serial
  Scenario: Phone OTP verification completes and updates completed KYC
    Given I complete phone OTP verification
    Then the response status is 200
    And no error occurred
    When I get the current user
    Then the response status is 200
    When I get completed KYC
    Then the response status is 200
    And completed KYC contains flow "phone_otp"
