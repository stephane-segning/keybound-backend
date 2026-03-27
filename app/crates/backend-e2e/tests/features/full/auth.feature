Feature: Authentication Enforcement
  Verify authentication is enforced on the remaining v2 surfaces

  Background:
    Given the e2e test environment is initialized
    And I have a valid authentication token
    And the database fixtures are set up

  @serial
  Scenario: BFF flow sessions require authentication
    When I send a POST request to /bff/sessions without authentication
    Then the response status is 401

  @serial
  Scenario: BFF flow sessions reject basic auth
    When I send a POST request to /bff/sessions with Basic auth
    Then the response status is 401

  @serial
  Scenario: BFF flow sessions reject invalid bearer
    When I send a POST request to /bff/sessions with an invalid Bearer token
    Then the response status is 401

  @serial
  Scenario: Staff flow steps require authentication
    When I send a GET request to /staff/flow/steps without authentication
    Then the response status is 401

  @serial
  Scenario: Valid authentication reaches the BFF flow surface
    When I send a POST request to /bff/sessions with valid authentication
    Then the response status is not 401

  @serial
  Scenario: Webhook step with valid Bearer auth succeeds
    Given the CUSS sink is reset
    When I create a session and start a webhook auth test flow with valid Bearer token
    Then the webhook auth test flow completes successfully

  @serial
  Scenario: Webhook step with invalid Bearer auth fails
    Given the CUSS sink is reset
    When I create a session and start a webhook auth test flow with invalid Bearer token
    Then the webhook auth test flow fails with authentication error

  @serial
  Scenario: Webhook step with Basic auth succeeds
    Given the CUSS sink is reset
    When I create a session and start a webhook basic auth test flow
    Then the webhook basic auth test flow completes successfully

  @serial
  Scenario: Webhook step with OAuth2 client credentials succeeds
    Given the CUSS sink is reset
    Given the OAuth2 token endpoint is configured
    When I create a session and start a webhook OAuth2 auth test flow
    Then the webhook OAuth2 auth test flow completes successfully
