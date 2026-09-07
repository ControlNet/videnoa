# Frontend Google Fonts dependency

- Both `web/index.html` and `controller-web/index.html` request a Google Fonts stylesheet from `fonts.googleapis.com` and preconnect to `fonts.gstatic.com`.
- The requested families are Manrope and Geist Mono. These are runtime browser requests, rather than locally bundled font assets.
- Source inspection found no Google login, analytics, reCAPTCHA, or application API dependency in either frontend. Controller API requests use the current page origin.
- Blocking Google can prevent the custom fonts from loading and may delay initial rendering while the external stylesheet request is pending. This does not itself establish a backend or iroh dependency on Google.
- To remove the external dependency, bundle the licensed fonts locally or remove the font links and use the existing fallback font stacks. No application change was requested or made during this inspection.

Recheck with `rg -n -i 'google|gstatic|googleapis' web/index.html controller-web/index.html web/src controller-web/src`.
