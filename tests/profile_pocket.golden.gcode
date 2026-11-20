%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- side contour (engine): Window with T5 ---)
(direction: climb — the material on the tool's right under M03)
(lead: 10.000mm along the path's normal, in air)
G00 Z25.000
G00 X32.071 Y32.071
G00 Z5.000
G01 Z-2.000 F300.000
G01 X25.000 Y25.000 F800.000
G01 X75.000 Y25.000
G01 X75.000 Y75.000
G01 X25.000 Y75.000
G01 X25.000 Y25.000
G01 X32.071 Y32.071
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
