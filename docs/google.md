# Google accounts

The Gemini API and a Google AI / Gemini app subscription have separate billing and
usage meters. This adapter adds separate dashboard bookmarks, not live usage polls.

```sh
token-manager add gemini --name gemini-api
token-manager add gemini-subscription --name personal
token-manager status --json
token-manager open gemini-api billing
token-manager open personal billing
```

Neither bookmark asks for credentials or makes account requests. Status reports
`unsupported`, with unavailable data rather than zero, and exits with code 2 as
for other unsupported providers. The API bookmark opens AI Studio billing; the
subscription bookmark opens Google One. View the app's meter in Gemini Settings.

No verified API-key balance/aggregate-spend endpoint or consumer subscription-quota
endpoint is implemented. Google Cloud billing exports and project monitoring need
a separate authenticated integration; they are not replaced by guessing a balance
from model pricing. Subscription credit promotions, where available, do not make
the app and API usage meters interchangeable.

Sources:
- https://aistudio.google.com/docs/billing
- https://support.google.com/gemini/answer/16275805
