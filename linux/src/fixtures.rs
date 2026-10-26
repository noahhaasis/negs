//! An explicit UI test fixture, not an engine parameter schema or engine defaults.
//! Replace the catalog/values with protocol data when the helper is connected.
pub struct Knob(
    pub &'static str,
    pub f64,
    pub f64,
    pub f64,
    pub f64,
    pub &'static str,
);
pub struct Group {
    pub name: &'static str,
    pub knobs: &'static [Knob],
}
pub const GROUPS: &[Group] = &[
    Group {
        name: "Camera",
        knobs: &[
            Knob("Exposure", -5., 5., 0.1, 0., "EV"),
            Knob("Film format", 4., 120., 0.1, 35., "mm"),
            Knob("Lens blur", 0., 100., 0.1, 0., "µm"),
        ],
    },
    Group {
        name: "Film Curves",
        knobs: &[
            Knob("Gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Fast gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Slow gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Red gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Green gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Blue gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Developer exhaustion", 0., 1., 0.01, 0., ""),
        ],
    },
    Group {
        name: "Grain",
        knobs: &[
            Knob("Dye clouds", 0., 5., 0.01, 2., "µm"),
            Knob("Post blur", 0., 3., 0.01, 0., "px"),
            Knob("Sharpen amount", 0., 3., 0.01, 1.5, ""),
            Knob("Sharpen radius", 0., 3., 0.01, 0.3, "px"),
            Knob("Clump blur", 0., 1., 0.01, 0.2, "µm"),
            Knob("Clump size", 0., 100., 1., 30., "nm"),
        ],
    },
    Group {
        name: "Halation",
        knobs: &[
            Knob("Amount", 0., 3., 0.01, 1., ""),
            Knob("Spatial scale", 0.1, 5., 0.01, 1., ""),
            Knob("Scatter amount", 0., 3., 0.01, 1., ""),
            Knob("Scatter scale", 0.1, 5., 0.01, 1., ""),
            Knob("Bounces", 1., 5., 1., 2., ""),
            Knob("Bounce decay", 0., 1., 0.01, 0.5, ""),
        ],
    },
    Group {
        name: "DIR Couplers",
        knobs: &[
            Knob("Amount", 0., 2., 0.01, 1., ""),
            Knob("Diffusion", 0., 100., 0.1, 10., "µm"),
            Knob("Tail", 0., 400., 1., 0., "µm"),
            Knob("Tail weight", 0., 1., 0.01, 0., ""),
        ],
    },
    Group {
        name: "Lens Diffusion",
        knobs: &[
            Knob("Strength", 0., 2., 0.01, 0.5, ""),
            Knob("Spatial scale", 0.1, 3., 0.01, 1., ""),
            Knob("Halo warmth", -1.5, 1.5, 0.01, 0., ""),
            Knob("Core intensity", 0., 2., 0.01, 1., ""),
            Knob("Halo intensity", 0., 2., 0.01, 1., ""),
            Knob("Bloom intensity", 0., 2., 0.01, 1., ""),
            Knob("Halo size", 0.1, 3., 0.01, 1., ""),
            Knob("Bloom size", 0.1, 3., 0.01, 1., ""),
        ],
    },
    Group {
        name: "Enlarger",
        knobs: &[
            Knob("Print exposure", 0.1, 5., 0.01, 1., ""),
            Knob("Magenta shift", -50., 50., 0.1, 0., ""),
            Knob("Yellow shift", -50., 50., 0.1, 0., ""),
            Knob("Preflash exposure", 0., 0.5, 0.01, 0., ""),
            Knob("Preflash magenta", -50., 50., 0.1, 0., ""),
            Knob("Preflash yellow", -50., 50., 0.1, 0., ""),
        ],
    },
    Group {
        name: "Print Curves",
        knobs: &[
            Knob("Gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Fast gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Slow gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Red gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Green gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Blue gamma", 0.5, 2., 0.01, 1., ""),
            Knob("Developer exhaustion", 0., 1., 0.01, 0., ""),
        ],
    },
    Group {
        name: "Glare",
        knobs: &[
            Knob("Amount", 0., 0.2, 0.001, 0.02, ""),
            Knob("Roughness", 0., 2., 0.01, 0.5, ""),
            Knob("Blur", 0., 5., 0.01, 1., "px"),
        ],
    },
    Group {
        name: "Scanner",
        knobs: &[
            Knob("Lens blur", 0., 5., 0.01, 0., "px"),
            Knob("Unsharp radius", 0., 3., 0.01, 0.7, "px"),
            Knob("Unsharp amount", 0., 2., 0.01, 0., ""),
            Knob("White level", 0.5, 1., 0.01, 1., ""),
            Knob("Black level", 0., 0.5, 0.01, 0., ""),
        ],
    },
];
pub const POST: &[Knob] = &[
    Knob("Brightness", -100., 100., 1., 0., ""),
    Knob("Contrast", -100., 100., 1., 0., ""),
    Knob("Whites", -100., 100., 1., 0., ""),
    Knob("Blacks", -100., 100., 1., 0., ""),
    Knob("Saturation", -100., 100., 1., 0., ""),
];

pub fn state() -> crate::model::State {
    let mut state = crate::model::State::default();
    for group in GROUPS {
        for k in group.knobs {
            state.values.insert(format!("{}.{}", group.name, k.0), k.4);
        }
    }
    for k in POST {
        state.values.insert(format!("Post.{}", k.0), k.4);
    }
    state
}
