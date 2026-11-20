%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- side contour (engine): Window with T5 [stock_to_leave 0.500mm, cusp budget 0.000mm] ---)
(direction: climb — the material on the tool's right under M03)
(lead: 10.000mm along the path's normal, in air)
G00 Z25.000
G00 X32.571 Y32.571
G00 Z5.000
G01 Z-2.000 F300.000
G01 X25.500 Y25.500 F800.000
G01 X74.500 Y25.500
G01 X74.500 Y74.500
G01 X25.500 Y74.500
G01 X25.500 Y25.500
G01 X32.571 Y32.571
G00 Z25.000
(--- side contour: Window with T5 [stock_to_leave 0.500mm, cusp budget 0.000mm] ---)
(direction: climb — the material on the tool's right under M03)
(lead: 10.000mm along the path's normal, in air)
G00 Z25.000
G00 X32.571 Y32.571
G00 Z5.000
G01 Z-2.000 F300.000
G01 G41 D5 X20.500 Y20.500 F800.000
G01 X79.500 Y20.500
G01 X79.500 Y79.500
G01 X20.500 Y79.500
G01 X20.500 Y20.500
G01 G40 X32.571 Y32.571
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
