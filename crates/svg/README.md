# filmcraft-svg

A bounded SVG subset parser, serializer and CPU rasterizer used by the native Legacy Title editor.

Supported shape elements: `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, and `path` (M/L/H/V/Q/C/Z path commands). Basic hex/named colors, opacity and stroke widths are supported. Scripts, external images, `<use>`, `foreignObject`, transforms and unsupported SVG constructs are not executed; the parser reports unsupported constructs as warnings.

The editor can open an SVG into this supported subset, edit canvas and shape properties, save the editable SVG, rasterize a PNG still, import it into the project, and attempt to place it at the start of V1. Text elements and full SVG 1.1 compatibility are not currently supported.
