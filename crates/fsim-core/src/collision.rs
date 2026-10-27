use crate::{
    prototypes::{Prototypes, Resistance, VehiclePrototype},
    vehicle::{CollisionEvent, Vehicles},
    world::World,
};

pub fn resisted_damage(raw: f64, resistances: &[Resistance]) -> f64 {
    let Some(r) = resistances.iter().find(|r| r.damage_type == "impact") else {
        return raw;
    };
    if raw <= 0.0 {
        return 0.0;
    }
    // Factorio wiki, Damage#Resistance. The fallback keeps small hits nonzero.
    let after_flat = if raw > r.decrease + 1.0 {
        raw - r.decrease
    } else if raw > 1.0 {
        1.0 / (r.decrease - raw + 2.0)
    } else {
        raw / (r.decrease + 1.0)
    };
    after_flat * (1.0 - r.percent / 100.0)
}

pub fn raw_to_destroy(health: f64, resistances: &[Resistance]) -> f64 {
    let Some(r) = resistances.iter().find(|r| r.damage_type == "impact") else {
        return health;
    };
    if r.percent >= 100.0 {
        return f64::INFINITY;
    }
    let needed = health / (1.0 - r.percent / 100.0);
    if needed >= 1.0 {
        needed + r.decrease
    } else if needed <= 1.0 / (r.decrease + 1.0) {
        needed * (r.decrease + 1.0)
    } else {
        r.decrease + 2.0 - 1.0 / needed
    }
}

// Returns whether the obstacle was destroyed. The caller still cancels this tick's motion.
pub fn impact(
    world: &mut World,
    vehicles: &mut Vehicles,
    i: usize,
    entity: usize,
    data: &Prototypes,
) -> bool {
    let p = &data.vehicles[vehicles.prototype[i]];
    let energy = p.weight * vehicles.speed[i].powi(2);
    impact_with_energy(world, vehicles, i, entity, data, energy)
}

pub(crate) fn impact_with_energy(
    world: &mut World,
    vehicles: &mut Vehicles,
    i: usize,
    entity: usize,
    data: &Prototypes,
    energy: f64,
) -> bool {
    let p = &data.vehicles[vehicles.prototype[i]];
    let available = energy / p.energy_per_hit_point;
    let mut destroyed = false;
    let raw = if let Some(prototype) = world.entities[entity].prototype {
        let resistance = &data.entities[prototype].resistances;
        let health = world.health(entity);
        let raw = available.min(raw_to_destroy(health, resistance));
        let mut health = ((health as f32 - resisted_damage(raw as f32 as f64, resistance) as f32)
            .max(0.0)) as f64;
        if health < 1e-9 {
            health = 0.0;
            destroyed = true;
        }
        world.set_health(entity, health);
        raw
    } else {
        available
    };
    let e = &world.entities[entity];
    let immune = immunity(p, &e.kind);
    let damage = if immune {
        0.0
    } else {
        resisted_damage(raw, &p.resistances)
    };
    vehicles.health[i] = (vehicles.health[i] as f32 - damage as f32).max(0.0) as f64;
    vehicles.last_collision[i] = Some(CollisionEvent {
        tick: vehicles.tick[i],
        entity_id: e.id,
        kind: e.kind.clone(),
        damage_taken: damage,
    });
    if destroyed {
        world.revision = world.revision.wrapping_add(1);
        let lost = raw * p.energy_per_hit_point;
        let remaining = (energy - lost).max(0.0);
        vehicles.speed[i] *= if energy > 0.0 {
            (remaining / energy).sqrt()
        } else {
            0.0
        };
    } else {
        vehicles.speed[i] = 0.0;
    }
    if vehicles.health[i] <= 0.0 {
        vehicles.speed[i] = 0.0;
    }
    destroyed
}

fn immunity(p: &VehiclePrototype, kind: &str) -> bool {
    if p.immune_to_all_impacts {
        return true;
    }
    match kind {
        "tree" => p.immune_to_tree_impacts,
        "rock" => p.immune_to_rock_impacts,
        "cliff" => p.immune_to_cliff_impacts,
        _ => false,
    }
}
