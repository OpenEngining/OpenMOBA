use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use asset::data::{Balance, ChampionData, GameData, QuickBuy, Spell, Targeting, UnitStats};
use asset::map::{
    Blending, CHAOS, Fountain, HeightField, MapData, MapDraw, MapGeometry, MapMaterial, MapObject,
    MapVertex, NavGrid, ORDER, ObjectKind,
};
use asset::{Animation, Character, Graph, Joint, Submesh, Transform, Vertex};
use image::{ImageBuffer, Rgba};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = match args.iter().position(|arg| arg == "--assets") {
        Some(index) if index + 1 < args.len() => PathBuf::from(&args[index + 1]),
        _ => PathBuf::from("web/assets"),
    };

    println!("Generating clean mock assets in {}...", out.display());

    fs::create_dir_all(&out)?;
    fs::create_dir_all(out.join("map"))?;
    fs::create_dir_all(out.join("champions"))?;
    fs::create_dir_all(out.join("units"))?;
    fs::create_dir_all(out.join("ui"))?;
    fs::create_dir_all(out.join("vfx"))?;
    fs::create_dir_all(out.join("audio"))?;

    create_1x1_png(&out.join("map/grass.png"), [60, 120, 60, 255])?;
    create_1x1_png(&out.join("champions/dummy.png"), [180, 180, 220, 255])?;

    generate_map_geometry(&out.join("map/map.bin"))?;
    generate_map_data(&out.join("map/data.bin"))?;
    generate_game_data(&out.join("data.bin"))?;
    generate_character(&out.join("champions/swordsman.bin"), "Swordsman")?;

    println!("Mock assets generated successfully.");
    Ok(())
}

fn create_1x1_png(path: &Path, color: [u8; 4]) -> Result<()> {
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(1, 1, Rgba(color));
    img.save(path)?;
    Ok(())
}

fn generate_map_geometry(path: &Path) -> Result<()> {
    let size = 2000.0;
    let vertices = vec![
        MapVertex {
            position: [-size, 0.0, -size],
            uv: [0.0, 0.0],
            lightmap_uv: [0.0, 0.0],
        },
        MapVertex {
            position: [size, 0.0, -size],
            uv: [1.0, 0.0],
            lightmap_uv: [1.0, 0.0],
        },
        MapVertex {
            position: [size, 0.0, size],
            uv: [1.0, 1.0],
            lightmap_uv: [1.0, 1.0],
        },
        MapVertex {
            position: [-size, 0.0, size],
            uv: [0.0, 1.0],
            lightmap_uv: [0.0, 1.0],
        },
    ];

    let indices = vec![0, 1, 2, 0, 2, 3];

    let materials = vec![MapMaterial {
        name: "ground".into(),
        texture: Some(0),
        tint: [0.4, 0.7, 0.4, 1.0],
        blending: Blending::Opaque,
        double_sided: false,
    }];

    let draws = vec![MapDraw {
        material: 0,
        lightmap: None,
        start: 0,
        count: 6,
    }];

    let geometry = MapGeometry {
        textures: vec!["grass.png".into()],
        materials,
        vertices,
        indices,
        draws,
        lightmap_scale: 1.0,
    };

    fs::write(path, geometry.to_bytes()?)?;
    Ok(())
}

fn generate_map_data(path: &Path) -> Result<()> {
    let map_data = MapData {
        bounds_min: [-1000.0, -1000.0],
        bounds_max: [1000.0, 1000.0],
        nav: NavGrid {
            min: [-1000.0, -1000.0],
            cell_size: 100.0,
            width: 20,
            height: 20,
            flags: vec![0; 400],
            heights: HeightField {
                min: [-1000.0, -1000.0],
                spacing: [100.0, 100.0],
                width: 20,
                height: 20,
                samples: vec![0.0; 400],
            },
        },
        fountains: vec![
            Fountain {
                team: ORDER,
                position: [-500.0, 0.0, -500.0],
            },
            Fountain {
                team: CHAOS,
                position: [500.0, 0.0, 500.0],
            },
        ],
        objects: vec![
            MapObject {
                kind: ObjectKind::Nexus,
                team: ORDER,
                position: [-400.0, 0.0, -400.0],
                facing: 0.0,
                record: "OrderNexus".into(),
                skin: "default".into(),
                lane: None,
                active_at_start: true,
            },
            MapObject {
                kind: ObjectKind::Nexus,
                team: CHAOS,
                position: [400.0, 0.0, 400.0],
                facing: std::f32::consts::PI,
                record: "ChaosNexus".into(),
                skin: "default".into(),
                lane: None,
                active_at_start: true,
            },
        ],
        barracks: Vec::new(),
        lane: vec![[-400.0, -400.0], [0.0, 0.0], [400.0, 400.0]],
    };

    fs::write(path, map_data.to_bytes()?)?;
    Ok(())
}

fn generate_game_data(path: &Path) -> Result<()> {
    let dummy_spell = |name: &str| Spell {
        name: name.into(),
        display_name: name.into(),
        targeting: Targeting::Direction,
        cast_range: vec![500.0],
        cooldown: vec![5.0],
        mana: vec![0.0],
        ..Spell::default()
    };

    let swordsman = ChampionData {
        name: "Swordsman".into(),
        display_name: "Swordsman".into(),
        title: "The Vanguard".into(),
        role: "Fighter".into(),
        stats: UnitStats {
            health: 620.0,
            health_regen: 8.0,
            move_speed: 340.0,
            attack_damage: 66.0,
            armor: 36.0,
            magic_resist: 32.0,
            attack_range: 175.0,
            attack_speed: 0.625,
            selection_radius: 65.0,
            pathing_radius: 35.0,
            gameplay_radius: 65.0,
            ..UnitStats::default()
        },
        slots: [
            "SwordsmanQ".into(),
            "SwordsmanW".into(),
            "SwordsmanE".into(),
            "SwordsmanR".into(),
        ],
        passive: dummy_spell("SwordsmanPassive"),
        spells: vec![
            dummy_spell("SwordsmanQ"),
            dummy_spell("SwordsmanW"),
            dummy_spell("SwordsmanE"),
            dummy_spell("SwordsmanR"),
        ],
        balance: Balance::default(),
        ..ChampionData::default()
    };

    let game_data = GameData {
        champions: vec![swordsman],
        units: Vec::new(),
        items: Vec::new(),
        summoners: Vec::new(),
        quick_buy: QuickBuy::default(),
    };

    fs::write(path, game_data.to_bytes()?)?;
    Ok(())
}

fn generate_character(path: &Path, name: &str) -> Result<()> {
    let s = 50.0;
    let vertices = vec![
        Vertex {
            position: [-s, 0.0, -s],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        },
        Vertex {
            position: [s, 0.0, -s],
            normal: [0.0, 1.0, 0.0],
            uv: [1.0, 0.0],
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        },
        Vertex {
            position: [s, 100.0, -s],
            normal: [0.0, 1.0, 0.0],
            uv: [1.0, 1.0],
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        },
        Vertex {
            position: [-s, 100.0, -s],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 1.0],
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];

    let character = Character {
        name: name.into(),
        stats: asset::Stats {
            move_speed: 340.0,
            attack_range: 175.0,
            selection_radius: 65.0,
            pathing_radius: 35.0,
        },
        scale: 1.0,
        self_illumination: 0.0,
        vertices,
        indices,
        submeshes: vec![Submesh {
            name: "body".into(),
            start: 0,
            count: 6,
            texture: 0,
            visible: true,
        }],
        textures: vec!["dummy.png".into()],
        joints: vec![Joint {
            name: "root".into(),
            parent: None,
            bind: Transform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
            },
            inverse_bind: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }],
        animations: vec![Animation {
            fps: 30.0,
            frames: vec![vec![Transform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
            }]],
        }],
        graph: Graph::default(),
    };

    fs::write(path, character.to_bytes()?)?;
    Ok(())
}
