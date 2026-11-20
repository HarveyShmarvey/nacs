%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- side bore wall (engine): Bore with T5 ---)
(direction: climb — the material on the tool's right under M03)
G00 Z25.000
(lead: from the bore's axis, in the air its clear left)
G00 X50.000 Y50.000
G01 Z-5.000 F300.000
G01 X65.000 Y50.000 F800.000
G03 X65.000 Y50.000 I-15.000 J0.000 F800.000
G00 Z25.000
(lead: from the bore's axis, in the air its clear left)
G00 X50.000 Y50.000
G01 Z-10.000 F300.000
G01 X65.000 Y50.000 F800.000
G03 X65.000 Y50.000 I-15.000 J0.000 F800.000
G00 Z25.000
(lead: from the bore's axis, in the air its clear left)
G00 X50.000 Y50.000
G01 Z-12.000 F300.000
G01 X65.000 Y50.000 F800.000
G03 X65.000 Y50.000 I-15.000 J0.000 F800.000
G00 Z25.000
(--- side bore wall (control): Bore with T5 ---)
(direction: climb — the material on the tool's right under M03)
G00 Z25.000
(lead: from one diameter inside the wall, in the air the bore's clear left)
G00 X60.000 Y50.000
G00 Z5.000
G01 Z-5.000 F300.000
G01 G41 D5 X70.000 Y50.000 F800.000
G03 X70.000 Y50.000 I-20.000 J0.000 F800.000
G01 G40 X60.000 Y50.000
G00 Z25.000
(lead: from one diameter inside the wall, in the air the bore's clear left)
G00 X60.000 Y50.000
G00 Z5.000
G01 Z-10.000 F300.000
G01 G41 D5 X70.000 Y50.000 F800.000
G03 X70.000 Y50.000 I-20.000 J0.000 F800.000
G01 G40 X60.000 Y50.000
G00 Z25.000
(lead: from one diameter inside the wall, in the air the bore's clear left)
G00 X60.000 Y50.000
G00 Z5.000
G01 Z-12.000 F300.000
G01 G41 D5 X70.000 Y50.000 F800.000
G03 X70.000 Y50.000 I-20.000 J0.000 F800.000
G01 G40 X60.000 Y50.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
