%
(nacs — nacs output)
(tool T5 dia 0.3937 -> comp register D5)
G20
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z0.9843
(--- side bore wall (engine): Bore with T5 ---)
(direction: climb — the material on the tool's right under M03)
G00 Z0.9843
(lead: from the bore's axis, in the air its clear left)
G00 X1.9685 Y1.9685
G01 Z-0.1969 F11.81
G01 X2.5591 Y1.9685 F31.50
G03 X2.5591 Y1.9685 I-0.5906 J0.0000 F31.50
G00 Z0.9843
(lead: from the bore's axis, in the air its clear left)
G00 X1.9685 Y1.9685
G01 Z-0.3937 F11.81
G01 X2.5591 Y1.9685 F31.50
G03 X2.5591 Y1.9685 I-0.5906 J0.0000 F31.50
G00 Z0.9843
(lead: from the bore's axis, in the air its clear left)
G00 X1.9685 Y1.9685
G01 Z-0.4724 F11.81
G01 X2.5591 Y1.9685 F31.50
G03 X2.5591 Y1.9685 I-0.5906 J0.0000 F31.50
G00 Z0.9843
(--- side bore wall (control): Bore with T5 ---)
(direction: climb — the material on the tool's right under M03)
G00 Z0.9843
(lead: from one diameter inside the wall, in the air the bore's clear left)
G00 X2.3622 Y1.9685
G00 Z0.1969
G01 Z-0.1969 F11.81
G01 G41 D5 X2.7559 Y1.9685 F31.50
G03 X2.7559 Y1.9685 I-0.7874 J0.0000 F31.50
G01 G40 X2.3622 Y1.9685
G00 Z0.9843
(lead: from one diameter inside the wall, in the air the bore's clear left)
G00 X2.3622 Y1.9685
G00 Z0.1969
G01 Z-0.3937 F11.81
G01 G41 D5 X2.7559 Y1.9685 F31.50
G03 X2.7559 Y1.9685 I-0.7874 J0.0000 F31.50
G01 G40 X2.3622 Y1.9685
G00 Z0.9843
(lead: from one diameter inside the wall, in the air the bore's clear left)
G00 X2.3622 Y1.9685
G00 Z0.1969
G01 Z-0.4724 F11.81
G01 G41 D5 X2.7559 Y1.9685 F31.50
G03 X2.7559 Y1.9685 I-0.7874 J0.0000 F31.50
G01 G40 X2.3622 Y1.9685
G00 Z0.9843
M09
M05
G00 Z0.9843
M30
%
