# ASVS 5.0 applicability and initial evidence review

Reviewed 2026-11-13 against application commit
`bfcd9cc5354f5efbca76a868a66fb3c63be2fc44` and the public `https://upyr.org/`
response observed at 11:50 UTC. This is a scoped internal review, **not an ASVS
L2 pass or certification**. The current stable standard has 345 requirements,
253 at levels 1–2; this document does not assert individual verification of all
253. Chapter applicability is not a compliance percentage.

Source: [OWASP ASVS 5.0.0](https://github.com/OWASP/ASVS/releases/tag/v5.0.0),
[official requirements CSV](https://github.com/OWASP/ASVS/blob/v5.0.0/5.0/docs_en/OWASP_Application_Security_Verification_Standard_5.0.0_en.csv).
CSV SHA-256: `98c8fe911b9edb403af8ee05d3ce8201ecac2659e313b053890a62847cdcf680`.
IDs below use that version, not Rapsody's older 4.0.3 numbering.

## Surfaces and trust boundaries

- **Desktop:** local keyboard/clipboard access, OS input permission, synthetic
  input, editable per-user TOML configuration and native/FFI integrations.
  Another process with the same user's privileges is outside the isolation
  provided by a per-user configuration file. Upyr has no application account,
  remote API, runtime OAuth flow or network update client.
- **Website/demo:** public static files and same-origin WASM. Typed demo text is
  processed in the browser. There is no project-operated backend, authenticated
  session, upload endpoint or database. Cloudflare's delivery configuration is a
  separate operational surface; a source review cannot verify all its settings.
- **Build services:** GitHub, Codecov and optional FOSSA process repository/build
  data. Their CI credentials are not application credentials. Build OIDC does
  not make ASVS's runtime OAuth requirements applicable to Upyr.

## Chapter applicability

“No surface” means the feature is absent from the reviewed product, not that its
controls passed. Revisit these decisions if the architecture changes.

| ASVS chapter | Desktop | Website/demo |
| --- | --- | --- |
| V1 Encoding and sanitization | Text/config parsing and native boundaries | DOM output and WASM boundary |
| V2 Validation and business logic | Config validation, correction state and limits | Input handling and resource limits |
| V3 Web frontend | No browser frontend | Applies |
| V4 API/web service | No service | Static HTTP response controls apply; no API |
| V5 File handling | Local config/import boundaries need desktop-specific review | No upload surface |
| V6 Authentication | OS identity/permissions; no app login | No login surface |
| V7 Sessions | No application session surface | No authenticated session surface |
| V8 Authorization | OS permission and per-user file access | Public content; no restricted resources |
| V9 Self-contained tokens | No runtime token surface | No runtime token surface |
| V10 OAuth/OIDC | No runtime OAuth/OIDC | No runtime OAuth/OIDC |
| V11 Cryptography | No application key store; OS security outside this review | Hosting TLS requires operational evidence |
| V12 Secure communication | No runtime network client | Applies to HTTPS delivery |
| V13 Configuration | Applies | Applies |
| V14 Data protection | Keyboard, clipboard, config and transient buffers | Demo text and browser storage |
| V15 Secure coding/architecture | Applies, including unsafe/FFI | Applies, including WASM |
| V16 Logging/error handling | Applies | Applies to client errors and hosting |
| V17 WebRTC | No WebRTC surface | No WebRTC surface |

## Selected control evidence

**Supported** is limited to the evidence described. **Partial** lacks complete
scope or assurance. **Gap** is an observed missing control/document. **Unverified**
needs additional inspection or testing. No row implies a whole chapter passed.

| ASVS 5.0 ID / focus | Result | Evidence and limit |
| --- | --- | --- |
| V3.2.2 / safe text output | Supported for demo paths | [demo.js](../../site/demo.js) uses `textContent` and input `.value`; the three `innerHTML` assignments in [keyboard.js](../../site/keyboard.js) use fixed strings. This is not an exhaustive DOM security proof or a CI sink-analysis guarantee. |
| V3.4.1 / HSTS | Gap | The observed public root response has no Strict-Transport-Security header. TLS availability alone does not satisfy this control. |
| V3.4.2 / CORS | Supported for public root | Observed `Access-Control-Allow-Origin: *`; response is public static HTML. No private response surface is exposed by this page. |
| V3.4.3 / CSP header | Partial | [HTML](../../site/index.html) and privacy tests enforce a CSP meta policy; the observed response lacks the required CSP **header**. |
| V3.4.4 / MIME sniffing | Partial | Root response has `X-Content-Type-Options: nosniff`. Other routes, redirects and error responses have not all been verified. |
| V3.4.5 / referrer policy | Partial | Observed `strict-origin-when-cross-origin`; demo text is not placed into URLs. This response sample does not establish all-route behavior. |
| V3.4.6 / framing | Gap | No `frame-ancestors` response directive was observed. `frame-src` in the HTML policy controls a different direction. |
| V12.1.1 / TLS versions | Unverified | HTTPS works; permitted protocol versions and cipher configuration were not exhaustively tested. |
| V13.1.1 / communication inventory | Supported for runtime | [Security policy](../../SECURITY.md) documents local runtime processing and same-origin site resources. The [privacy guard](../../tools/check_privacy.py) checks enumerated patterns and required site policies. CI service connections are documented separately. |
| V14.1.1–V14.1.2 / data classification | Partial | Keyboard text, clipboard and configuration boundaries are documented; a complete classification/retention matrix is still absent. |
| V14.2.3 / third-party disclosure | Partial | Source/dependency checks reject only the crate names and native/web API patterns listed in [the privacy guard](../../tools/check_privacy.py); unlisted integrations may pass. No packet-level or hostile-host investigation was performed. Hosting behavior is outside the source guard. |
| V14.3.3 / browser storage | Supported for reviewed demo | Source inspection of [app.js](../../site/app.js), [demo.js](../../site/demo.js) and site modules found no localStorage/sessionStorage/IndexedDB/cookie writes of typed text. This source snapshot does not establish behavior of browser extensions or the host browser. |
| V15.1.1 / remediation deadlines | Gap | [SECURITY.md](../../SECURITY.md) defines acknowledgment timing and explicit maintenance exceptions, but no severity-based fix deadlines. |
| V15.1.3–V15.2.2 / resource limits | Partial | [Engine contract](../../tools/run_engine_contract.py) checks cache/mapping budgets. [Config loading](../../src/config.rs) reads the whole file before parsing; there is no file-size cap. A same-user malformed-file/availability review remains open. |
| V16.1.1 / logging inventory | Partial | Logging/privacy rules exist; the full OS log locations, access controls and retention inventory has not been verified. |
| V16.2.5 / sensitive logs | Partial | Privacy guard and regression cases reject direct sensitive log fields. Transitive errors, native dependencies and OS logging still need review. |
| V1.4.3 / resource lifetime | Unverified | Rust ownership and tested cleanup paths help, but unsafe/FFI paths require a dedicated lifetime/error-path audit. |

## Desktop evidence beyond the web standard

- [Config writer](../../src/config.rs): exclusive temporary-file creation,
  atomic replacement and Unix `0600`; regression tests cover permissions and
  failed replacement. Windows inherits per-user ACLs; this review did not perform
  an adversarial multi-user Windows ACL test.
- [Clipboard guard](../../src/clipboard_guard.rs) and
  [automation input guard](../../src/automation/input_guard.rs): explicit resource
  restoration and interference boundaries. Existing tests are useful evidence;
  this review did not drive the user's desktop or certify every editor.
- [Accessibility gate](../../src/accessibility.rs): uses platform permission
  checks. Permission revocation, secure-field behavior and cross-process races
  need their existing platform tests and separate live evidence.

## Follow-up work

1. Add and verify HTTP security headers across pages, assets, redirects and errors;
   decide the HSTS subdomain scope before asserting the level-2 requirement.
2. Define data/log retention and dependency-remediation policies with realistic
   maintainer commitments.
3. Review and bound config/import sizes; audit unsafe/FFI failure paths and native
   permission/clipboard behavior with synthetic fixtures.
4. Complete a requirement-by-requirement assessment of the applicable level-1/2
   controls, retaining evidence and explicit exclusions, before any level claim.

The open items are part of the result, not suppressed exceptions. Re-run this
review after relevant architecture, hosting or permission changes.
