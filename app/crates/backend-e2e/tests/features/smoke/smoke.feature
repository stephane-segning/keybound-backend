Feature: Smoke Tests
  Basic health checks to verify all services are running

  @serial
  Scenario: All services are reachable and healthy
    Given the e2e test environment is initialized
    And the user-storage service is reachable within 60 seconds
    And the keycloak service is reachable within 60 seconds
    And the cuss service is reachable within 60 seconds
    And the sms-sink service is reachable within 60 seconds
    When I reset the SMS sink
    Then all services are healthy
