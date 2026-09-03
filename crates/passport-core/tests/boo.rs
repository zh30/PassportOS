//! Host tests for Boo. Mic is an 8-bit level through the shipped gate, not I2S.

use passport_core::api::{MemoryStore, MeteredDraw, NullDraw};
use passport_core::board::{Key, LCD_H, LCD_W, TYPICAL_OK_MV};
use passport_core::boo::{
    BEST_KEY, BooState, BooWorld, COL_BG, COL_FACE, COL_MUTE, FACE_H, FACE_W, GHOST_H, GHOST_W,
    MUTE_BAR_H, MUTE_BAR_W, MUTE_TICKS, MobKind, SCARE_X, SCORE_H, SCORE_W, TWO_LANE_AT, WORLD_H,
    WORLD_W, in_scare, is_boo_lane_key, is_boo_roar_key, live_budget_holds, read_best, write_best,
};
use passport_core::compositor::{LIVE_SPI_BUDGET, Rect, STATUS_BAR_H, rgb565_bytes};
use passport_core::flap::{Redraw, cadence_redraw};
use passport_core::input::{ButtonDecoder, ButtonEvent};
use passport_core::mic::{LOUD_LEVEL, MicEvent, MicGate, ROAR_LEVEL, pcm16_le_level};
use passport_core::theme::Palette;

fn vp() -> Rect {
    Rect {
        x: 0,
        y: STATUS_BAR_H,
        w: LCD_W,
        h: LCD_H - STATUS_BAR_H,
    }
}

fn spi_of(world: &BooWorld, mode: Redraw) -> u32 {
    let mut n = NullDraw::new(vp());
    let mut m = MeteredDraw::new(&mut n);
    world.paint(&mut m, vp(), mode, Palette::DARK);
    m.spi_bytes()
}

#[test]
fn pcm16_silence_is_zero_loud_is_high() {
    assert_eq!(pcm16_le_level(&[]), 0);
    assert_eq!(pcm16_le_level(&[0, 0, 0, 0, 0, 0, 0, 0]), 0);
    let mut loud = [0u8; 16];
    for c in loud.chunks_exact_mut(2) {
        c.copy_from_slice(&i16::MAX.to_le_bytes());
    }
    assert!(
        pcm16_le_level(&loud) > 200,
        "peak PCM must map high, got {}",
        pcm16_le_level(&loud)
    );
}

#[test]
fn mic_gate_edges_once_per_yell() {
    let mut g = MicGate::new();
    assert_eq!(g.feed(10, ROAR_LEVEL, LOUD_LEVEL), None);
    assert_eq!(g.feed(60, ROAR_LEVEL, LOUD_LEVEL), Some(MicEvent::Loud));
    assert_eq!(g.feed(90, ROAR_LEVEL, LOUD_LEVEL), None);
    assert_eq!(g.feed(10, ROAR_LEVEL, LOUD_LEVEL), None);
    assert_eq!(g.feed(210, ROAR_LEVEL, LOUD_LEVEL), Some(MicEvent::Peak));
    assert_eq!(g.feed(220, ROAR_LEVEL, LOUD_LEVEL), None);
}

#[test]
fn ready_shout_or_ok_starts() {
    let mut w = BooWorld::new(1);
    assert_eq!(w.state, BooState::Ready);
    w.feed_mic(80);
    assert_eq!(w.state, BooState::Playing);
    let mut w = BooWorld::new(2);
    w.roar(false);
    assert_eq!(w.state, BooState::Playing);
}

#[test]
fn roar_in_window_scores_and_removes_ghost() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(SCARE_X + 2, 0, MobKind::Ghost);
    assert!(in_scare(SCARE_X + 2));
    w.mark_painted();
    w.roar(false);
    assert_eq!(w.score, 1);
    assert!(w.mobs().is_empty());
    assert_eq!(w.state, BooState::Playing);
}

#[test]
fn roar_too_early_mutes_and_second_roar_is_ignored() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(WORLD_W - 20, 0, MobKind::Ghost);
    assert!(!in_scare(WORLD_W - 20));
    w.roar(false);
    assert_eq!(w.score, 0);
    assert!(w.is_muted());
    assert_eq!(w.mobs().len(), 1);
    w.roar(false);
    assert_eq!(w.score, 0, "muted yell must not scare");
    for _ in 0..MUTE_TICKS {
        w.tick();
        if w.state == BooState::Dead {
            break;
        }
    }
}

#[test]
fn ghost_reaching_the_face_is_dead() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(SCARE_X - GHOST_W, 0, MobKind::Ghost);
    w.tick();
    assert_eq!(w.state, BooState::Dead);
}

#[test]
fn king_in_window_plus_roar_is_dead() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(SCARE_X + 2, 0, MobKind::King);
    w.roar(false);
    assert_eq!(w.state, BooState::Dead);
}

#[test]
fn king_may_pass_if_you_stay_quiet() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(SCARE_X - GHOST_W, 0, MobKind::King);
    w.tick();
    assert_eq!(w.state, BooState::Playing);
    assert!(w.mobs().is_empty());
}

#[test]
fn too_loud_with_king_on_screen_is_dead() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(WORLD_W - 30, 0, MobKind::King);
    w.roar(true);
    assert_eq!(w.state, BooState::Dead);
}

#[test]
fn wrong_lane_roar_mutes_after_two_lanes() {
    let mut w = BooWorld::new(1);
    w.start();
    w.score = TWO_LANE_AT;
    w.set_lane(0);
    w.place(SCARE_X + 2, 1, MobKind::Ghost);
    w.roar(false);
    assert_eq!(w.score, TWO_LANE_AT);
    assert!(w.is_muted());
    assert_eq!(w.mobs().len(), 1);
    w.nudge_lane(ButtonEvent::Press(Key::Down));
    assert_eq!(w.lane, 1);
}

#[test]
fn dead_ok_resets_keeping_best() {
    let mut w = BooWorld::new(1);
    w.start();
    w.score = 6;
    w.best = 6;
    w.state = BooState::Dead;
    w.roar(false);
    assert_eq!(w.state, BooState::Ready);
    assert_eq!(w.score, 0);
    assert_eq!(w.best, 6);
}

#[test]
fn best_uses_own_store_key() {
    let mut store = MemoryStore::new();
    write_best(&mut store, 9);
    assert_eq!(BEST_KEY, b"boo");
    assert_eq!(read_best(&store), 9);
}

#[test]
fn ok_press_from_decoder_is_roar_key() {
    let mut dec = ButtonDecoder::new();
    let mut events = heapless::Vec::<ButtonEvent, 8>::new();
    for _ in 0..4 {
        events.extend(dec.feed(TYPICAL_OK_MV, 20));
    }
    assert!(
        events.iter().copied().any(is_boo_roar_key),
        "OK press must roar, got {events:?}"
    );
    assert!(is_boo_lane_key(ButtonEvent::Press(Key::Up)));
    assert!(is_boo_lane_key(ButtonEvent::Click(Key::Down)));
}

#[test]
fn ready_is_full_once_then_playing_is_live() {
    let mut w = BooWorld::new(1);
    assert_eq!(w.redraw(), Redraw::Full);
    w.mark_painted();
    assert_eq!(w.redraw(), Redraw::None);
    w.start();
    assert_eq!(w.redraw(), Redraw::Full);
    w.mark_painted();
    assert_eq!(w.redraw(), Redraw::Live);
}

#[test]
fn live_tick_stays_in_spi_budget() {
    assert!(live_budget_holds());
    let mut w = BooWorld::new(1);
    w.start();
    w.place(120, 0, MobKind::Ghost);
    w.place(180, 1, MobKind::King);
    w.mark_painted();
    w.tick();
    let live = w.live_spi_bytes();
    assert!(
        live <= LIVE_SPI_BUDGET,
        "live {live} vs budget {LIVE_SPI_BUDGET}"
    );
    let painted = spi_of(&w, Redraw::Live);
    assert!(painted <= LIVE_SPI_BUDGET, "paint live {painted}");
    let full = rgb565_bytes(WORLD_W as u16, WORLD_H as u16);
    assert!(full > LIVE_SPI_BUDGET);
}

#[test]
fn live_scroll_only_paints_ghost_strips() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(120, 0, MobKind::Ghost);
    w.mark_painted();
    w.tick();
    let ops = w.live_ops();
    assert!(
        !ops.iter()
            .any(|o| o.rect.w == GHOST_W as u16 && o.rect.h == GHOST_H as u16),
        "scroll must not erase+redraw the whole ghost, ops={ops:?}"
    );
    assert!(
        !ops.iter()
            .any(|o| o.rect.w == FACE_W as u16 && o.rect.h == FACE_H as u16),
        "unchanged face must not be a live blit, ops={ops:?}"
    );
    let live = w.live_spi_bytes();
    let full_body = rgb565_bytes(GHOST_W as u16, GHOST_H as u16).saturating_mul(2);
    assert!(
        live < full_body,
        "strip paint {live} must beat erase+draw body {full_body}"
    );
    assert_eq!(cadence_redraw(Redraw::Live, false), Redraw::None);
}

#[test]
fn live_scroll_skies_old_ghost_only() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(100, 0, MobKind::Ghost);
    w.mark_painted();
    w.tick();
    let ops = w.live_ops();
    assert!(
        ops.iter()
            .any(|o| o.rgb565 == COL_BG && o.rect.w <= GHOST_W as u16 + 2),
        "must erase vacated ghost, ops={ops:?}"
    );
}

/// Device order: mark_painted → apply_notes (roar) → tick → live paint.
#[test]
fn live_after_roar_skies_scared_ghost_and_score() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(SCARE_X + 2, 0, MobKind::Ghost);
    w.mark_painted();
    w.roar(false);
    assert_eq!(w.score, 1);
    assert!(w.mobs().is_empty());
    w.tick();
    assert_eq!(w.redraw(), Redraw::Live);
    let ops = w.live_ops();
    assert!(
        ops.iter().any(|o| o.rgb565 == COL_BG
            && o.rect.w == GHOST_W as u16
            && o.rect.h == GHOST_H as u16),
        "scared ghost must live-erase, ops={ops:?}"
    );
    assert!(
        ops.iter().any(|o| o.rgb565 == COL_BG
            && o.rect.w == SCORE_W as u16
            && o.rect.h == SCORE_H as u16),
        "score plate must live-fill, ops={ops:?}"
    );
    let painted = spi_of(&w, Redraw::Live);
    let ghost_erase = rgb565_bytes(GHOST_W as u16, GHOST_H as u16);
    assert!(
        painted >= ghost_erase,
        "live paint must include scared ghost sky, {painted}"
    );
    assert!(
        painted <= LIVE_SPI_BUDGET,
        "live {painted} vs budget {LIVE_SPI_BUDGET}"
    );
}

/// Device order: mark_painted → roar miss → tick → mute bar live-paints.
#[test]
fn live_after_miss_shows_mute_bar() {
    let mut w = BooWorld::new(1);
    w.start();
    w.place(WORLD_W - 20, 0, MobKind::Ghost);
    w.mark_painted();
    w.roar(false);
    assert!(w.is_muted());
    w.tick();
    assert_eq!(w.redraw(), Redraw::Live);
    let ops = w.live_ops();
    assert!(
        ops.iter().any(|o| o.rgb565 == COL_MUTE
            && o.rect.w == MUTE_BAR_W as u16
            && o.rect.h == MUTE_BAR_H as u16),
        "mute bar must live-paint, ops={ops:?}"
    );
    let painted = spi_of(&w, Redraw::Live);
    assert!(painted > 0);
    assert!(painted <= LIVE_SPI_BUDGET);
}

/// Device order: mark_painted → nudge_lane → tick → face hops live.
#[test]
fn live_after_nudge_moves_face() {
    let mut w = BooWorld::new(1);
    w.start();
    w.score = TWO_LANE_AT;
    w.set_lane(0);
    w.place(120, 0, MobKind::Ghost);
    w.mark_painted();
    w.nudge_lane(ButtonEvent::Press(Key::Down));
    assert_eq!(w.lane, 1);
    w.tick();
    assert_eq!(w.redraw(), Redraw::Live);
    let ops = w.live_ops();
    let faces: heapless::Vec<_, 8> = ops
        .iter()
        .filter(|o| o.rect.w == FACE_W as u16 && o.rect.h == FACE_H as u16)
        .copied()
        .collect();
    assert_eq!(faces.len(), 2, "old face sky + new face, ops={ops:?}");
    assert!(
        faces.iter().any(|o| o.rgb565 == COL_BG),
        "must sky previous lane, ops={ops:?}"
    );
    assert!(
        faces.iter().any(|o| o.rgb565 == COL_FACE),
        "must draw new lane, ops={ops:?}"
    );
    let painted = spi_of(&w, Redraw::Live);
    let face_pair = rgb565_bytes(FACE_W as u16, FACE_H as u16).saturating_mul(2);
    assert!(
        painted >= face_pair,
        "live paint must include both faces, {painted}"
    );
    assert!(painted <= LIVE_SPI_BUDGET);
}
