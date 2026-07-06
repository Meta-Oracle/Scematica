import { isNative, apiFetch } from './net'

// Registers the device for push on native and hands its FCM token to the paired
// instance (`POST /api/push/register`). No-op on web. The Capacitor plugin is imported
// dynamically so the web bundle never eagerly loads native code. Requires the Android
// project to be configured with Firebase (google-services.json) — see docs/mobile-app.md.
export async function initPush(): Promise<void> {
  if (!isNative()) return
  try {
    const { PushNotifications } = await import('@capacitor/push-notifications')

    let perm = await PushNotifications.checkPermissions()
    if (perm.receive === 'prompt' || perm.receive === 'prompt-with-rationale') {
      perm = await PushNotifications.requestPermissions()
    }
    if (perm.receive !== 'granted') return

    await PushNotifications.addListener('registration', async (t) => {
      await apiFetch('/api/push/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token: t.value, platform: 'android' }),
      }).catch(() => {})
    })

    await PushNotifications.register()
  } catch {
    /* plugin unavailable (e.g. Firebase not configured) — degrade silently */
  }
}
