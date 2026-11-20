%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- side contour wire (engine): Rim with T5 ---)
(direction: climb — the material on the tool's right under M03)
(lead: 10.000mm along the path's normal — the stock model has no claim here)
G00 Z25.000
G00 X32.071 Y32.071
G00 Z5.000
G01 Z-3.167 F300.000
G01 X25.000 Y25.000 F800.000
G01 X75.000 Y25.000 Z-4.833
G01 X75.000 Y75.000 Z-3.167
G01 X25.000 Y75.000 Z-4.833
G01 X25.000 Y25.000 Z-3.167
G01 X32.071 Y32.071
G00 Z25.000
(--- side contour wire: Rim with T5 ---)
(direction: climb — the material on the tool's right under M03)
(lead: 10.000mm along the path's normal — the stock model has no claim here)
G00 Z25.000
G00 X32.071 Y32.071
G00 Z5.000
G01 Z-3.000 F300.000
G01 G41 D5 X20.000 Y20.000 F800.000
G01 X80.000 Y20.000 Z-5.000
G01 X80.000 Y80.000 Z-3.000
G01 X20.000 Y80.000 Z-5.000
G01 X20.000 Y20.000 Z-3.000
G01 G40 X32.071 Y32.071
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
