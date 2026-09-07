# Local font storage estimate

Measured actual Google Fonts WOFF2 response bodies on 2026-09-07 using a modern Chrome user agent and the variable-font request for Geist Mono weights 400..600 and Manrope weights 300..800. These ranges cover the union of both frontend HTML requests. Fonts were downloaded into memory for measurement; no font assets were added to the repository.

| Family | Latin subset | All returned subsets |
| --- | ---: | ---: |
| Geist Mono | 23,128 bytes | 70,448 bytes |
| Manrope | 24,836 bytes | 74,972 bytes |
| Total | 47,964 bytes (46.8 KiB) | 145,420 bytes (142.0 KiB) |

- Add a few KiB for CSS declarations and retained font license notices; these estimates are font payload sizes, not measured final archive/image deltas.
- Recommended full returned subset coverage costs about 0.15 MB per independently bundled frontend, or about 0.30 MB when both Worker and Controller distributions each contain a copy.
- Latin-only coverage costs about 0.05 MB per frontend, but omits the other supplied character subsets.
- Variable WOFF2 files cover the requested weights without needing one file per weight. Full returned coverage includes Latin/Latin Extended, Cyrillic/Cyrillic Extended, Vietnamese, and the family-specific Greek/symbols subsets. It does not add CJK fonts; Chinese text continues to use system fallback fonts.
- Source CSS endpoint: `https://fonts.googleapis.com/css2?family=Geist+Mono:wght@400..600&family=Manrope:wght@300..800&display=swap`. Measured versions were Geist Mono v6 and Manrope v20. Byte sizes may change if the provider updates assets or a different font distribution is used.
