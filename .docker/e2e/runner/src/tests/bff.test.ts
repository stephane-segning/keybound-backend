import { describe, expect, test } from 'vitest';
import { ensureBffFixtures } from '../db';
import { env } from '../env';
import { getJson, sendJson } from '../http';
import { getClientTokenAndSubject } from '../keycloak';
import { resetSmsSink, waitForOtpMessage } from '../sms';

const bffBase = `${env.userStorageUrl}/bff`;
const staffBase = `${env.userStorageUrl}/staff`;

function requireValue<T>(value: T | undefined | null, message: string): T {
  if (value === undefined || value === null) {
    throw new Error(message);
  }
  return value;
}

async function authContext() {
  const { token, subject } = await getClientTokenAndSubject();
  await ensureBffFixtures(subject);
  return {
    userId: subject,
    headers: {
      Authorization: `Bearer ${token}`,
    },
  };
}

describe('BFF flows', () => {
  test('creates and retrieves phone deposit', async () => {
    const { headers, userId } = await authContext();
    const depositResponse = await sendJson<{
      depositId: string;
      status: string;
      contact?: { phoneNumber?: string };
    }>({
      url: `${bffBase}/internal/deposits/phone`,
      method: 'POST',
      headers,
      body: {
        userId,
        amount: 150_000,
        currency: 'XAF',
        provider: 'MTN_CM',
        reason: 'e2e test',
      },
    });

    expect(depositResponse.statusCode).toBe(201);
    const depositId = requireValue(
      depositResponse.body?.depositId,
      'depositId should be present',
    );

    const lookup = await sendJson({
      url: `${bffBase}/internal/deposits/${depositId}`,
      method: 'GET',
      headers,
    });

    expect(lookup.statusCode).toBe(200);
    expect(lookup.body?.status).toBe('CONTACT_PROVIDED');
    expect(lookup.body?.contact?.phoneNumber).toBeTruthy();
  });

  test('issues and verifies phone OTP via admin sink', async () => {
    const { headers, userId } = await authContext();
    await resetSmsSink();

    const sessionRes = await sendJson<{ id: string }>({
      url: `${bffBase}/internal/kyc/sessions`,
      method: 'POST',
      headers,
      body: { userId },
    });
    const sessionId = requireValue(sessionRes.body?.id, 'session id required');

    const stepRes = await sendJson<{ id: string }>({
      url: `${bffBase}/internal/kyc/steps`,
      method: 'POST',
      headers,
      body: {
        sessionId,
        userId,
        type: 'PHONE',
        policy: {},
      },
    });
    const stepId = requireValue(stepRes.body?.id, 'step id required');

    const msisdn = '+237690000033';
    const issueRes = await sendJson<{ otpRef: string }>({
      url: `${bffBase}/internal/kyc/phone/otp/issue`,
      method: 'POST',
      headers,
      body: {
        stepId,
        msisdn,
        channel: 'SMS',
        ttlSeconds: 120,
      },
    });
    const otpRef = requireValue(issueRes.body?.otpRef, 'otpRef is required');

    const message = await waitForOtpMessage(msisdn, 60, 500);
    expect(message.otp).toBeDefined();

    const verify = await sendJson({
      url: `${bffBase}/internal/kyc/phone/otp/verify`,
      method: 'POST',
      headers,
      body: {
        stepId,
        otpRef,
        code: message.otp,
      },
    });

    expect(verify.statusCode).toBe(200);
  }, 40_000);
});

describe('Staff surface', () => {
  test('reports summary and instances respond', async () => {
    const { headers } = await authContext();

    const summary = await sendJson<{
      byKind: Record<string, number>;
      byStatus: Record<string, number>;
      failuresLast24h: number;
    }>({
      url: `${staffBase}/api/kyc/reports/summary`,
      method: 'GET',
      headers,
    });

    expect(summary.statusCode).toBe(200);
    expect(summary.body?.byKind).toBeDefined();
    expect(summary.body?.byStatus).toBeDefined();
    expect(typeof summary.body?.failuresLast24h).toBe('number');

    const instances = await sendJson<{
      items: unknown[];
      total: number;
      page: number;
      pageSize: number;
    }>({
      url: `${staffBase}/api/kyc/instances`,
      method: 'GET',
      headers,
    });

    expect(instances.statusCode).toBe(200);
    expect(Array.isArray(instances.body?.items)).toBe(true);

    const missing = await sendJson({
      url: `${staffBase}/api/kyc/instances/missing-instance`,
      method: 'GET',
      headers,
    });
    expect(missing.statusCode).toBe(404);
  });
});

describe('Stubbed integrations', () => {
  test('cuss stub records register calls', async () => {
    const payload = {
      firstName: 'E2E',
      lastName: 'Runner',
      phone: '+237690000044',
      externalId: `cuss-${Date.now()}`,
    };

    const cussPost = await sendJson({
      url: `${env.cussUrl}/api/registration/register`,
      method: 'POST',
      body: payload,
    });

    expect(cussPost.statusCode).toBe(201);

    const recorded = await getJson<Array<{ endpoint: string }>>(
      `${env.cussUrl}/__admin/requests`,
    );
    expect(recorded.some((entry) => entry.endpoint === 'register')).toBe(true);
  });
});
