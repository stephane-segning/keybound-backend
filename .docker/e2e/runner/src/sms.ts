import { env } from './env';
import { getJson, sendJson, sleep } from './http';

export interface OtpMessage {
  id: string;
  phone: string;
  otp: string;
  timestamp: string;
}

export async function resetSmsSink() {
  await sendJson({ url: `${env.smsSinkUrl}/__admin/reset`, method: 'POST', body: {} });
}

export async function waitForOtpMessage(
  phone: string,
  attempts = 20,
  intervalMs = 500,
): Promise<OtpMessage> {
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    const messages = await getJson<OtpMessage[]>(`${env.smsSinkUrl}/__admin/messages`);
    const message = messages.find((entry) => entry.phone === phone);
    if (message) {
      return message;
    }

    await sleep(intervalMs);
  }

  throw new Error(`OTP for ${phone} not received within ${attempts} attempts`);
}
