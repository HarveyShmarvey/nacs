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
(pattern: spine at 0.000deg, 14 rows at 6.000)
(direction: the rows run both ways, climb and conventional by turns)
G00 Z25.000
(plunge: in air outside the blank)
G00 X-5.000 Y0.000
G00 Z5.000
G01 Z-0.500 F300.000
G01 X125.000 Y0.000 F800.000
G01 X125.000 Y6.000
G01 X-5.000 Y6.000
G01 X-5.000 Y12.000
G01 X125.000 Y12.000
G01 X125.000 Y18.000
G01 X-5.000 Y18.000
G01 X-5.000 Y24.000
G01 X125.000 Y24.000
G01 X125.000 Y30.000
G01 X-5.000 Y30.000
G01 X-5.000 Y36.000
G01 X125.000 Y36.000
G01 X125.000 Y42.000
G01 X-5.000 Y42.000
G01 X-5.000 Y48.000
G01 X125.000 Y48.000
G01 X125.000 Y54.000
G01 X-5.000 Y54.000
G01 X-5.000 Y60.000
G01 X125.000 Y60.000
G01 X125.000 Y66.000
G01 X-5.000 Y66.000
G01 X-5.000 Y72.000
G01 X125.000 Y72.000
G01 X125.000 Y78.000
G01 X-5.000 Y78.000
G00 Z5.000
G00 Z25.000
(--- pocket clear: Notch with T5 ---)
(open edges: 2 of 4 — entered from air)
(pattern: held under 90deg, the default: 11 rows at 5.000, the spacing the ceiling derives on T5, on spine's comb at 90.000deg, then 1 shore pass along what stands; 5 lifts where a link would exceed the ceiling; over it: 20.000mm on the rows, 4.991mm on the shores)
(direction: the rows run both ways, climb and conventional by turns; the passes along the walls climb)
G00 Z25.000
(plunge: in air outside the blank)
G00 X0.000 Y84.993
G00 Z5.000
G01 Z-20.000 F300.000
G01 X0.000 Y45.000 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 5.000mm off it)
G00 X5.000 Y45.000
G00 Z5.000
G01 Z-20.000 F300.000
G01 X5.000 Y85.000 F800.000
G01 X10.000 Y85.000
G01 X10.000 Y45.000
G00 Z5.000
(plunge: beside a stroke already cut, 5.000mm off it)
G00 X15.000 Y45.000
G00 Z5.000
G01 Z-20.000 F300.000
G01 X15.000 Y85.000 F800.000
G01 X20.000 Y85.000
G01 X20.000 Y45.000
G00 Z5.000
(plunge: beside a stroke already cut, 5.000mm off it)
G00 X25.000 Y45.000
G00 Z5.000
G01 Z-20.000 F300.000
G01 X25.000 Y85.000 F800.000
G01 X30.000 Y85.000
G01 X30.000 Y45.000
G00 Z5.000
(plunge: beside a stroke already cut, 5.000mm off it)
G00 X35.000 Y45.000
G00 Z5.000
G01 Z-20.000 F300.000
G01 X35.000 Y85.000 F800.000
G01 X40.000 Y85.000
G01 X40.000 Y45.000
G00 Z5.000
(plunge: beside a stroke already cut, 5.000mm off it)
G00 X45.000 Y45.000
G00 Z5.000
G01 Z-20.000 F300.000
G01 X45.000 Y85.000 F800.000
G01 X50.000 Y85.000
G01 X50.000 Y45.000
G01 X-5.000 Y45.000
G01 X0.000 Y45.000
G01 X0.100 Y45.000
G01 X55.000 Y45.000
G01 X55.000 Y79.900
G01 X55.000 Y80.000
G01 X55.000 Y85.000
G00 Z5.000
G00 Z25.000
(--- side contour: Notch with T5 ---)
(direction: climb — the material on the tool's right under M03)
(wall 1 of 1: 2 edges, open at both ends)
(lead: 10.000mm along the path's own line, back from its start, in air)
G00 Z25.000
G00 X-10.000 Y45.000
G00 Z5.000
G01 Z-20.000 F300.000
G01 G41 D5 X0.000 Y40.000 F800.000
G01 X60.000 Y40.000
G01 X60.000 Y80.000
G01 G40 X55.000 Y90.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
