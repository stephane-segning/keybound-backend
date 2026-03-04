function requireEnv(key: string): string {
  const value = process.env[key];
  if (!value) {
    throw new Error(`environment variable ${key} is required`);
  }
  return value;
}

export const env = {
  userStorageUrl: requireEnv('USER_STORAGE_URL'),
  keycloakUrl: requireEnv('KEYCLOAK_URL'),
  cussUrl: requireEnv('CUSS_URL'),
  smsSinkUrl: requireEnv('SMS_SINK_URL'),
  databaseUrl: requireEnv('DATABASE_URL'),
  kcSignatureSecret: requireEnv('KC_SIGNATURE_SECRET'),
  keycloakClientId: requireEnv('KEYCLOAK_CLIENT_ID'),
  keycloakClientSecret: requireEnv('KEYCLOAK_CLIENT_SECRET'),
};
