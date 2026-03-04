import { Client } from 'pg';
import { env } from './env';

const STAFF_USER_ID = 'usr_e2e_staff_001';
const STAFF_PHONE = '+237690000001';

export async function ensureBffFixtures(userId: string): Promise<void> {
  const client = new Client({ connectionString: env.databaseUrl });
  await client.connect();

  try {
    await client.query(
      `
      DELETE FROM sm_instance
      WHERE user_id = $1
      `,
      [userId],
    );

    await client.query(
      `
      INSERT INTO app_user (
        user_id,
        realm,
        username,
        disabled,
        created_at,
        updated_at
      ) VALUES ($1, 'e2e-testing', $2, false, NOW(), NOW())
      ON CONFLICT (user_id) DO UPDATE
      SET
        realm = EXCLUDED.realm,
        username = EXCLUDED.username,
        disabled = false,
        updated_at = NOW()
      `,
      [userId, `subject-${userId}`],
    );

    await client.query(
      `
      INSERT INTO app_user (
        user_id,
        realm,
        username,
        first_name,
        last_name,
        phone_number,
        disabled,
        created_at,
        updated_at
      ) VALUES (
        $1,
        'staff',
        'e2e-staff',
        'E2E',
        'Staff',
        $2,
        false,
        NOW(),
        NOW()
      )
      ON CONFLICT (user_id) DO UPDATE
      SET
        realm = EXCLUDED.realm,
        username = EXCLUDED.username,
        first_name = EXCLUDED.first_name,
        last_name = EXCLUDED.last_name,
        phone_number = EXCLUDED.phone_number,
        disabled = false,
        updated_at = NOW()
      `,
      [STAFF_USER_ID, STAFF_PHONE],
    );
  } finally {
    await client.end();
  }
}
