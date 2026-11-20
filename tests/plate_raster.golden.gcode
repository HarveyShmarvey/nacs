%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- floor: TopFace with T5 ---)
(direction: the rows run both ways, climb and conventional by turns)
(plunge: in air off the blank's edge)
G00 Z25.000
G00 X-5.000 Y0.000
G00 Z5.000
G01 Z-0.500 F300.000
G01 X125.000 F800.000
G01 Y6.000
G01 X-5.000
G01 Y12.000
G01 X125.000
G01 Y18.000
G01 X-5.000
G01 Y24.000
G01 X125.000
G01 Y30.000
G01 X-5.000
G01 Y36.000
G01 X125.000
G01 Y42.000
G01 X-5.000
G01 Y48.000
G01 X125.000
G01 Y54.000
G01 X-5.000
G01 Y60.000
G01 X125.000
G01 Y66.000
G01 X-5.000
G01 Y72.000
G01 X125.000
G01 Y78.000
G01 X-5.000
G01 Y80.000
G01 X125.000
G00 Z25.000
(--- side contour: Profile1 with T5 ---)
(direction: climb — the material on the tool's right under M03)
(wall 1 of 1: 2 edges, open at both ends)
(lead: 10.000mm along the path's own line, back from its start, in air)
G00 Z25.000
G00 X65.000 Y90.000
G00 Z5.000
G01 Z-0.500 F300.000
G01 G41 D5 X60.000 Y80.000 F800.000
G01 X60.000 Y40.000
G01 X0.000 Y40.000
G01 G40 X-10.000 Y35.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
