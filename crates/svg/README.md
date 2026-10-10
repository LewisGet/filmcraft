# filmcraft-svg

Implements a deliberately bounded subset of SVG 1.1 Second Edition, using the W3C SVG 1.1 Second Edition specification as the format reference. This crate is original FilmCraft code and does not use SVG renderer implementation source code.

The initial implementation supports basic shapes, solid colours, opacity, and a conservative subset of path commands. Unsupported elements and attributes are reported as warnings. External references and DTDs are not loaded.
