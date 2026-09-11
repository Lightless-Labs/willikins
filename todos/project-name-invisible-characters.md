# ProjectName: reject invisible and bidi characters

**Filed:** 2026-09-11 (task 2 verification)

`ProjectName` rejects only `char::is_control()`. U+00A0 (no-break space), U+200B (zero-width
space), U+202E (right-to-left override), U+2066..U+2069, U+FEFF and U+00AD survive into
display names, and display names reach rendered templates such as CLAUDE.md, where a bidi
override can disguise content. Reject Unicode format characters (category Cf) and the
bidi controls, and fold U+00A0 to a plain space before trimming. Scheduled for task 4, which
touches the types crate anyway.
