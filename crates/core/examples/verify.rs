//! Compare the engine with SeedmapOracle output (tools/oracle).
//! usage: verify ORACLE.txt SEED DIMENSION RADIUS
use seedmap_core::structures::{candidates, catalog, strongholds};
use seedmap_core::*;
use std::collections::{BTreeMap, BTreeSet};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let seed: i64 = a[2].parse().unwrap();
    let dim = match a[3].as_str() {
        "the_nether" => Dimension::Nether,
        "the_end" => Dimension::End,
        _ => Dimension::Overworld,
    };
    let r: i32 = a[4].parse().unwrap();
    let d = data();
    let w = World::new(WorldSpec::new(seed, dim));
    let mut s = w.sampler();
    s.exact_heights = std::env::var("FAST").is_err();
    let (mut ok, mut bad) = (0, 0);
    let mut starts: BTreeMap<String, BTreeMap<(i32, i32), String>> = BTreeMap::new();
    let mut fails: BTreeMap<String, BTreeSet<(i32, i32)>> = BTreeMap::new();
    let mut rings = Vec::new();
    for line in text.lines() {
        let p: Vec<&str> = line.split(' ').collect();
        match p[0] {
            "biome" => {
                let q = [
                    p[1].parse().unwrap(),
                    p[2].parse().unwrap(),
                    p[3].parse().unwrap(),
                ];
                let got = d.biome_name(s.biome(q));
                if got == p[4] {
                    ok += 1
                } else {
                    bad += 1;
                    if bad < 5 {
                        println!("biome {q:?}: vanilla {} ours {got}", p[4]);
                    }
                }
            }
            "spawn" => println!("spawn vanilla {} {} ours {:?}", p[1], p[2], w.spawn()),
            "ring" => rings.push((p[2].parse::<i32>().unwrap(), p[3].parse::<i32>().unwrap())),
            "start" => {
                starts
                    .entry(p[1].into())
                    .or_default()
                    .insert((p[2].parse().unwrap(), p[3].parse().unwrap()), p[4].into());
            }
            "fail" => {
                fails
                    .entry(p[1].into())
                    .or_default()
                    .insert((p[2].parse().unwrap(), p[3].parse().unwrap()));
            }
            _ => {}
        }
    }
    println!("biomes: {ok} match, {bad} differ");
    if !rings.is_empty() {
        let ours: Vec<(i32, i32)> = strongholds(&w).iter().map(|f| f.chunk).collect();
        let same = rings.iter().zip(&ours).filter(|(a, b)| a == b).count();
        println!("strongholds: {same}/{} ring positions match", rings.len());
    }
    let cat = catalog();
    for set in cat.sets_in(dim) {
        if set.spacing.is_none() {
            continue;
        }
        let cands = candidates(seed, set.index, -r, -r, r, r);
        let vs = starts.get(&set.name).cloned().unwrap_or_default();
        let vf = fails.get(&set.name).cloned().unwrap_or_default();
        let placement_ok = cands.len() == vs.len() + vf.len()
            && cands.iter().all(|c| vs.contains_key(c) || vf.contains(c));
        let (mut tp, mut fp, mut fneg, mut variant) = (0, 0, 0, 0);
        for c in &cands {
            let found = s.validate(set.index, c.0, c.1);
            match (found, vs.get(c)) {
                (Some(f), Some(name)) => {
                    tp += 1;
                    if cat.structures[f.structure].name != *name {
                        variant += 1;
                    }
                }
                (Some(_), None) => fp += 1,
                (None, Some(_)) => fneg += 1,
                (None, None) => {}
            }
        }
        println!(
            "{:<28} placement {} | vanilla {:>5} starts | ok {:>5} false+ {:>4} missed {:>4} wrong-variant {}",
            set.name,
            if placement_ok { "exact" } else { "DIFF" },
            vs.len(),
            tp,
            fp,
            fneg,
            variant
        );
    }
}
