# Third-party notices

OpenADE is licensed under Apache-2.0 (see [LICENSE](LICENSE)). The
following components include material derived from other projects:

## Merge0 (MIT)

The normalized **Signal schema** (v0.7), the **fingerprint** construction
(`<source>:<hex16>` over length-prefixed SHA-256 parts), the **inbox /
triage model** (dismissal-reason taxonomy, dismissal impact snapshots,
escalation reopen), the **outcome-memory design** (fingerprint-anchored
outcomes with idempotent writes, age-marked/STALE prior-attempt
rendering), and the **named-omissions** context-budget rule used in
`crates/openade-server` and `crates/openade-core` are derived from
[Merge0](https://github.com/hkd987/Merge0):

```
MIT License

Copyright (c) 2026 AdminRemix LLC

Permission is hereby granted, free of charge, to any person obtaining a
copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be included
in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

Nothing from Merge0's `ee/` directory (commercial license) is included.
