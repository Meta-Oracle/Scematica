import { isNative } from './net'

// Push notifications are OPT-IN and intentionally NOT bundled in the default build.
//
// The `@capacitor/push-notifications` plugin pulls in Firebase Messaging, whose
// `FirebaseInitProvider` runs at process startup and requires a valid
// `google-services.json` / `google_app_id`. Bundling it WITHOUT that config makes the
// app **crash instantly on launch**. Since push needs a per-operator Firebase project
// anyway, we ship a crash-free default without it and let you turn it on deliberately.
//
// To enable push (see docs/mobile-app.md → "Push notifications"):
//   1. `npm i @capacitor/push-notifications`
//   2. Add your Firebase `google-services.json` to `web/android/app/`.
//   3. Set `FCM_SERVICE_ACCOUNT` on the instance running scematica-api.
//   4. Replace the body of initPush() below with the real implementation:
//
//        if (!isNative()) return
//        const { PushNotifications } = await import('@capacitor/push-notifications')
//        let perm = await PushNotifications.checkPermissions()
//        if (perm.receive === 'prompt' || perm.receive === 'prompt-with-rationale')
//          perm = await PushNotifications.requestPermissions()
//        if (perm.receive !== 'granted') return
//        await PushNotifications.addListener('registration', async (t) => {
//          await apiFetch('/api/push/register', { method: 'POST',
//            headers: { 'Content-Type': 'application/json' },
//            body: JSON.stringify({ token: t.value, platform: 'android' }) }).catch(() => {})
//        })
//        await PushNotifications.register()
//
//   5. `npm run mobile:apk` and reinstall.
export async function initPush(): Promise<void> {
  if (!isNative()) return
  // no-op in the default build — push is opt-in (see the comment above).
}
