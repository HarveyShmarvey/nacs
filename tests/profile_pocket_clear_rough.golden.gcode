%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- pocket clear: PocketProfile with T5 [stock_to_leave 0.500mm, cusp budget 0.000mm] ---)
(direction: climb — the material on the tool's right under M03)
G00 Z25.000
G00 X49.500 Y49.500
G00 Z5.000
(plunge: this loop is 4.000mm round, under one diameter — no room to ramp)
G01 Z-5.000 F300.000
G01 X50.500 Y49.500 F800.000
G01 X50.500 Y50.500
G01 X49.500 Y50.500
G01 X49.500 Y49.500
G00 Z5.000
(plunge: beside air an earlier loop cut, 4.000mm off its path)
G00 X45.500 Y45.500
G01 Z-5.000 F300.000
G01 X54.500 Y45.500 F800.000
G01 X54.500 Y54.500
G01 X45.500 Y54.500
G01 X45.500 Y45.500
G00 Z5.000
(plunge: beside air an earlier loop cut, 4.000mm off its path)
G00 X41.500 Y41.500
G01 Z-5.000 F300.000
G01 X58.500 Y41.500 F800.000
G01 X58.500 Y58.500
G01 X41.500 Y58.500
G01 X41.500 Y41.500
G00 Z5.000
(plunge: beside air an earlier loop cut, 4.000mm off its path)
G00 X37.500 Y37.500
G01 Z-5.000 F300.000
G01 X62.500 Y37.500 F800.000
G01 X62.500 Y62.500
G01 X37.500 Y62.500
G01 X37.500 Y37.500
G00 Z5.000
(plunge: beside air an earlier loop cut, 4.000mm off its path)
G00 X33.500 Y33.500
G01 Z-5.000 F300.000
G01 X66.500 Y33.500 F800.000
G01 X66.500 Y66.500
G01 X33.500 Y66.500
G01 X33.500 Y33.500
G00 Z5.000
(plunge: beside air an earlier loop cut, 4.000mm off its path)
G00 X29.500 Y29.500
G01 Z-5.000 F300.000
G01 X70.500 Y29.500 F800.000
G01 X70.500 Y70.500
G01 X29.500 Y70.500
G01 X29.500 Y29.500
G00 Z5.000
(plunge: beside air an earlier loop cut, 4.000mm off its path)
G00 X25.500 Y25.500
G01 Z-5.000 F300.000
G01 X74.500 Y25.500 F800.000
G01 X74.500 Y74.500
G01 X25.500 Y74.500
G01 X25.500 Y25.500
G00 Z5.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
