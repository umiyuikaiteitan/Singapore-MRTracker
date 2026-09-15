// Singapore MRT: TWO INDEPENDENT DISPLAY ITEMS. Units are millimetres.
// LED carrier: laser-cut 420x298x3 mm front/back, separated by 34 mm.
// E-paper: independent 188x136 mm shell; never fitted into the LED front.
// Render one selected part with: openscad -D 'part="pixel_cup"' -o part.stl enclosures.scad
$fn=48;
part="epaper_shell";

module spacer(height=34, diameter=10, bore=3.3) {
    difference() {
        cylinder(h=height,d=diameter);
        translate([0,0,-.1]) cylinder(h=height+.2,d=bore);
    }
}

module pixel_cup() {
    // LED shines down through the opening at z=0. Tape the flat bottom to the
    // front panel rear, aligned with its 5.5 mm aperture. Insert module from +Z.
    difference() {
        translate([-6.3,-6.3,0]) cube([12.6,12.6,5.5]);
        translate([-5.1,-5.1,1.2]) cube([10.2,10.2,4.4]);
        translate([0,0,-.1]) cylinder(h=1.4,d=5.7);
        // Cable exit at rear edge; do not clamp wire against the PCB.
        translate([4.9,-2,3]) cube([1.5,4,2.6]);
    }
}

module led_parts() {
    for(i=[0:5]) translate([i*14,0,0]) spacer();
    for(i=[0:15]) translate([(i%8)*16,24+floor(i/8)*16,0]) pixel_cup();
}

module epaper_shell() {
    // Screen glass sits directly behind a SEPARATE 3mm laser front bezel.
    // Support its perimeter with foam tape; no pressure on active glass.
    // Shell interior 182x130, intentionally loose to allow alignment by eye
    // to the 164.2x99mm aperture and strain relief for the FPC cable.
    difference() {
        union() {
            difference() {
                cube([188,136,28]);
                translate([3,3,3]) cube([182,130,26]);
            }
            for(x=[5,183],y=[5,131]) translate([x,y,3]) cylinder(h=25,d=7);
        }
        for(x=[5,183],y=[5,131]) translate([x,y,3]) cylinder(h=25.2,d=2.6);
        // Cable opening in side. This is a clearance opening for a USB cable,
        // not a fitted connector: measured DevKit connector positions vary.
        translate([-1,105,8]) cube([5,14,9]);
        for(x=[64:10:124]) translate([x,127,-.1]) cube([3,2,3.2]);
    }
}

module epaper_front() {
    difference() {
        cube([188,136,3]);
        translate([11.9,18.5,-.1]) cube([164.2,99,3.2]);
        for(x=[5,183],y=[5,131]) translate([x,y,-.1]) cylinder(h=3.2,d=3.3);
    }
}

module epaper_blank_mount() {
    // Universal electronics mounting plate; adhesive/Velcro holds a DevKit
    // and the matching e-paper driver board without guessed PCB hole patterns.
    difference() {
        cube([90,48,2]);
        for(x=[10,80]) translate([x,24,-.1]) cylinder(h=2.2,d=3.3);
        for(x=[20,30,60,70]) translate([x,3,-.1]) cube([3,42,2.2]);
    }
}

if(part=="pixel_cup") pixel_cup();
else if(part=="panel_spacer") spacer();
else if(part=="pcb_spacer") spacer(12,7,3.3);
else if(part=="led_parts") led_parts();
else if(part=="epaper_shell") epaper_shell();
else if(part=="epaper_front") epaper_front();
else if(part=="epaper_mount") epaper_blank_mount();
else assert(false,"Unknown part selection");
