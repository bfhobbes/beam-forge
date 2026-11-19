"""Writes ezdxf_mtext.dxf: MTEXT (and a TEXT) placed at known points.

No CAD tool on hand writes MTEXT (FreeCAD's exporters only write TEXT), so
this file comes from the ezdxf library instead: it stands in for a CAD
export, and the reference positions in tests/dxf_import.rs come from the
definitions below, not from what a CAD program displayed. Rotation is set
both ways real files carry it: group 50 in degrees (as LibreCAD and QCAD
write it) and an X-axis direction, group 11 (as AutoCAD writes it).

    pip install ezdxf==1.4.5
    python ezdxf_mtext.py <dir for the .dxf>
"""

import sys
from pathlib import Path

import ezdxf
from ezdxf.enums import MTextEntityAlignment as A

doc = ezdxf.new("R2000", units=4)  # $INSUNITS 4: millimetres
doc.styles.add("Plotter", font="romans.shx")
msp = doc.modelspace()
msp.add_lwpolyline([(0, 0), (100, 0), (100, 60), (0, 60)], close=True)

# Top left: the first line's cap top at the insertion point.
msp.add_mtext("Top left", dxfattribs={"char_height": 5, "style": "Plotter"}).set_location(
    (10, 50), attachment_point=A.TOP_LEFT
)
# Middle centre, two lines with a paragraph break and inline formatting.
msp.add_mtext(
    "{\\fArial|b0|i0;Mid}\\Pline", dxfattribs={"char_height": 4, "width": 0}
).set_location((50, 30), attachment_point=A.MIDDLE_CENTER)
# Bottom right.
msp.add_mtext("BR", dxfattribs={"char_height": 3}).set_location(
    (90, 10), attachment_point=A.BOTTOM_RIGHT
)
# Rotated 90 degrees by group 50.
up = msp.add_mtext("Up", dxfattribs={"char_height": 4, "rotation": 90})
up.set_location((95, 5), attachment_point=A.BOTTOM_LEFT)
# Rotated 90 degrees by an X-axis direction.
axis = msp.add_mtext("Axis", dxfattribs={"char_height": 4, "text_direction": (0, 1, 0)})
axis.set_location((5, 5), attachment_point=A.BOTTOM_LEFT)
# Wraps to 30 mm (in two lines, in most sans fonts).
msp.add_mtext("wraps at thirty mm", dxfattribs={"char_height": 3, "width": 30}).set_location(
    (10, 40), attachment_point=A.TOP_LEFT
)
# A TEXT with a degree sign, centred on its second point.
msp.add_text("45%%d", dxfattribs={"height": 3}).set_placement(
    (50, 55), align=ezdxf.enums.TextEntityAlignment.CENTER
)

out = Path(sys.argv[1]) / "ezdxf_mtext.dxf"
doc.saveas(out)
print(out)
