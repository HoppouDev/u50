# Fixture provenance and licensing

The golden fixtures under `u50_style/tests/fixtures/<lang>/` are derived from
real-world open-source projects. Each `dirty.<ext>` was produced by minifying
(compacting) the upstream source; each `expected.<ext>` is the fixed point of
`style50 -o format` (see the _Golden fixture tests_ section in
`u50_style/AGENTS.md` for the generation procedure). Upstream licenses are
reproduced verbatim below the per-fixture details.

| Fixture           | Upstream project                                          | Upstream source                                          | License                                                                        | Minification                                                                            |
| ----------------- | --------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `c/dirty.c`       | [cJSON](https://github.com/DaveGamble/cJSON)              | `cJSON.c`                                                | MIT (SPDX: `MIT`) — Copyright (c) 2009-2017 Dave Gamble and cJSON contributors | `/* */` comments stripped, whitespace-only lines removed, non-preprocessor lines joined |
| `cpp/dirty.cpp`   | [JsonCpp](https://github.com/open-source-parsers/jsoncpp) | `src/lib_json/json_reader.cpp`                           | MIT (SPDX: `MIT`) — Baptiste Lepilleur and The JsonCpp Authors                 | `//` and `/* */` comments stripped, leading license header preserved, lines joined      |
| `java/dirty.java` | [Guava](https://github.com/google/guava)                  | `guava/src/com/google/common/collect/ImmutableList.java` | Apache-2.0 (SPDX: `Apache-2.0`) — Copyright (C) 2007 The Guava Authors         | comments stripped, Apache license header preserved, lines joined                        |
| `py/dirty.py`     | [Werkzeug](https://github.com/pallets/werkzeug)           | `src/werkzeug/routing/map.py` at tag `3.0.3`             | BSD-3-Clause (SPDX: `BSD-3-Clause`) — Copyright 2007 Pallets                   | comment lines and blank lines removed (AST-validated output)                            |
| `js/dirty.js`     | [jQuery](https://github.com/jquery/jquery)                | official `jquery.min.js` v3.7.1                          | MIT (SPDX: `MIT`) — Copyright OpenJS Foundation and other contributors         | upstream-published minified build used as-is                                            |
| `css/dirty.css`   | [normalize.css](https://github.com/necolas/normalize.css) | `normalize.css`                                          | MIT (SPDX: `MIT`) — Copyright © Nicolas Gallagher and Jonathan Neal            | comments stripped, whitespace collapsed to single spaces                                |
| `html/dirty.html` | [Bootstrap](https://github.com/twbs/bootstrap)            | Astro-built `docs/5.3/examples/dashboard/` page          | MIT (SPDX: `MIT`) — Copyright (c) 2011-2026 The Bootstrap Authors              | leading indentation stripped from alternating lines (djhtml restores it)                |
| `sql/dirty.sql`   | [Supabase](https://github.com/supabase/supabase)          | migrations for the `nimbus`/`page` tables                | Apache-2.0 (SPDX: `Apache-2.0`) — Supabase, Inc.                               | `--` comments stripped, whitespace collapsed to single spaces                           |

The minification is deterministic whitespace/comment removal only; no code
semantics were altered. All fixture content remains subject to the upstream
licenses listed below, reproduced verbatim from the projects' license files.

---

## cJSON (MIT)

```
Copyright (c) 2009-2017 Dave Gamble and cJSON contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

## JsonCpp (MIT)

Reproduced from the upstream `LICENSE` file (`jsoncpp.LICENSE` in the
generation workspace); JsonCpp is distributed under the MIT license with
copyright attributed to Baptiste Lepilleur and The JsonCpp Authors. See
https://github.com/open-source-parsers/jsoncpp/blob/master/LICENSE for the
authoritative text.

## Guava (Apache-2.0)

The fixture carries the upstream Apache License 2.0 header verbatim
("Copyright (C) 2007 The Guava Authors"). The full Apache License 2.0 text
applies; see https://www.apache.org/licenses/LICENSE-2.0.

## Werkzeug (BSD-3-Clause)

Reproduced from the upstream `LICENSE.txt` (Copyright 2007 Pallets), the
standard 3-clause BSD license. See
https://github.com/pallets/werkzeug/blob/main/LICENSE.txt for the
authoritative text.

## jQuery (MIT)

Reproduced from the upstream `LICENSE.txt` (Copyright OpenJS Foundation and
other contributors), the standard MIT license. See
https://github.com/jquery/jquery/blob/main/LICENSE.txt for the authoritative
text.

## normalize.css (MIT)

Reproduced from the upstream `LICENSE.md` (Copyright © Nicolas Gallagher and
Jonathan Neal), the standard MIT license. See
https://github.com/necolas/normalize.css/blob/master/LICENSE.md for the
authoritative text.

## Bootstrap (MIT)

Reproduced from the upstream `LICENSE` (Copyright (c) 2011-2026 The Bootstrap
Authors), the standard MIT license. See
https://github.com/twbs/bootstrap/blob/main/LICENSE for the authoritative
text.

## Supabase (Apache-2.0)

The full Apache License 2.0 text applies to the upstream migrations; see
https://www.apache.org/licenses/LICENSE-2.0.
