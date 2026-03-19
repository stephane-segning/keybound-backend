Feature: Flow User Endpoints
  Verify user reads and completed KYC behavior on the v2 flow surface

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
Scenario: Initial completed KYC is empty
    When I get completed KYC
    Then the response status is 200
    And completed KYC is empty

@serial
Scenario: Phone OTP verification updates completed KYC
    Given I complete phone OTP verification
    When I get completed KYC
    Then the response status is 200
    And completed KYC contains flow "phone_otp"
