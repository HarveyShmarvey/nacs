%
(nacs — nacs output)
(tool T5 dia 12.700 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.400
(--- side bore wall (engine): Bore with T5 ---)
(direction: climb — the material on the tool's right under M03)
G00 Z25.400
(lead: from the bore's axis, in the air its clear left)
G00 X50.800 Y50.800
G01 Z-5.080 F304.800
G01 X63.500 Y50.800 F812.800
G03 X63.500 Y50.800 I-12.700 J0.000 F812.800
G00 Z25.400
(lead: from the bore's axis, in the air its clear left)
G00 X50.800 Y50.800
G01 Z-10.160 F304.800
G01 X63.500 Y50.800 F812.800
G03 X63.500 Y50.800 I-12.700 J0.000 F812.800
G00 Z25.400
(lead: from the bore's axis, in the air its clear left)
G00 X50.800 Y50.800
G01 Z-12.700 F304.800
G01 X63.500 Y50.800 F812.800
G03 X63.500 Y50.800 I-12.700 J0.000 F812.800
G00 Z25.400
M09
M05
G00 Z25.400
M30
%
