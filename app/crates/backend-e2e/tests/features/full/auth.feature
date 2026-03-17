Feature: Authentication Enforcement
  Verify authentication is enforced on the remaining v2 surfaces

  Background:
    Given the e2e test environment is initialized
    And I have a valid authentication token
    And the database fixtures are set up

  @serial
  Scenario: BFF flow sessions require authentication
    When I send a POST request to /bff/flow/sessions without authentication
    Then the response status is 401

  @serial
  Scenario: BFF flow sessions reject basic auth
    When I send a POST request to /bff/flow/sessions with Basic auth
    Then the response status is 401

  @serial
  Scenario: BFF flow sessions reject invalid bearer
    When I send a POST request to /bff/flow/sessions with an invalid Bearer token
    Then the response status is 401

  @serial
  Scenario: Staff flow steps require authentication
    When I send a GET request to /staff/flow/steps without authentication
    Then the response status is 401

  @serial
  Scenario: Valid authentication reaches the BFF flow surface
    When I send a POST request to /bff/flow/sessions with valid authentication
    Then the response status is not 401
