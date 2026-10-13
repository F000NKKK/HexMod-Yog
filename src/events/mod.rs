//! Event handlers for hex casting interactions.
use yog_api::content::StartupGrant;
use yog_api::{EventPhase, Registry};

use crate::casting::vm;
use crate::protocol::{CH_CAST, CH_OPEN, CH_RESET, CH_STACK};
use crate::ui::casting_screen;

pub fn register(registry: &mut Registry) {
    // Grant the Hexbook and a staff on first join.
    registry.register_startup_grant(
        StartupGrant::new("hexcasting:give_hexbook").item("hexcasting:thehexbook"),
    );
    registry.register_startup_grant(
        StartupGrant::new("hexcasting:give_staff").item("hexcasting:staff/oak"),
    );

    // Packets carry player names; the VM needs uuids for entity iotas.
    registry.on_player_join(|ev, phase, _srv| {
        if phase == EventPhase::Post {
            vm::remember_player(&ev.player_name, &ev.uuid);
        }
        true
    });

    // Right-clicking the staff (server side) tells that client to open the
    // grid. Sneak + right-click resets the caster's stack first — the stack
    // itself always lives on the server and survives reopening the grid.
    registry.on_use_item(|ev, phase, srv| {
        if phase == EventPhase::Post && ev.item_id.starts_with("hexcasting:staff/") {
            srv.send_to_player(&ev.player_name, CH_OPEN, b"");
            if ev.sneaking {
                vm::reset(&ev.player_name, srv);
            }
        }
        true
    });

    // Server: a drawn pattern arrives — run it through the casting VM.
    registry.on_packet(CH_CAST, |ev, srv| {
        vm::handle_cast(&ev.player, &ev.payload, srv);
    });

    // Server: the screen's Clear button.
    registry.on_packet(CH_RESET, |ev, srv| {
        vm::reset(&ev.player, srv);
    });

    // Client: open the casting screen on a fresh grid.
    registry.on_client_packet(CH_OPEN, |_ev, _srv| {
        casting_screen::open_session();
    });

    // Client: the server's answer to a pattern.
    registry.on_client_packet(CH_STACK, |ev, _srv| {
        casting_screen::on_report(&ev.payload);
    });
}
