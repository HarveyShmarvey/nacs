%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- side contour (engine): Lprofile with T5 ---)
(direction: climb — the material on the tool's right under M03)
(wall 1 of 1: 2 edges, open at both ends)
(lead: 10.000mm along the path's own line, back from its start, in air)
G00 Z25.000
G00 X65.000 Y90.000
G00 Z5.000
G01 Z-0.500 F300.000
G01 X65.000 Y80.000 F800.000
G01 X65.000 Y40.000
G02 X60.000 Y35.000 I-5.000 J0.000 F800.000
G01 X0.000 Y35.000
G01 X-10.000 Y35.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
