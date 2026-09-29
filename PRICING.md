# Hikari pricing

Open core, paid Pro. Still images are free forever; motion and documents pay
for development. No seat counting, no render metering on self-hosted use.

## Tiers

| | Free | Pro ($19/mo or $190/yr) | Scale (custom) |
|---|---|---|---|
| PNG/SVG stills | ✓ | ✓ | ✓ |
| MIT/Apache-2.0 core | ✓ | ✓ | ✓ |
| Animated GIF | — | ✓ | ✓ |
| Animated APNG | — | ✓ | ✓ |
| Lossless WebP stills | — | ✓ | ✓ |
| Animated WebP | — | roadmap (needs libwebp C or unverified API) | ✓ |
| PDF documents | — | ✓ | ✓ |
| SaaS render API + SLA | — | — | ✓ |
| Redistribution in closed products | — | — | ✓ |
| Priority support | community | email, 2 business days | Slack, 1 business day |

## How licensing works

- Pro keys are `ed25519`-signed strings (`hk1.…`), verified offline in
  `hikari-license`. No network calls, no telemetry, no metering.
- Enforcement is the key check plus the commercial terms — the check lives in
  source you can read, exactly like Sidekiq. We state that openly instead of
  shipping DRM theater.
- `License::dev()` exists for evaluation and tests. Production needs a key
  from the signing service (run `hikari-license` mint on your infra).

## Why pay

Free covers the 90% OG-image case with no strings attached. Pro funds the
expensive parts: PDF conformance (PDF/A, PDF/UA validation), font licensing
and subsetting work, prebuilt binaries for every OS/arch, and the WASM diet.
