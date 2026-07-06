//! Event handlers for hex casting interactions.
use yog_api::{EventPhase, Registry};
use yog_api::content::StartupGrant;

use crate::casting::vm;
use crate::grid;

pub fn register(registry: &mut Registry) {
    // Grant the Hexbook and a staff on first join.
    registry.register_startup_grant(
        StartupGrant::new("hexcasting:give_hexbook")
            .item("hexcasting:thehexbook")
    );
    registry.register_startup_grant(
        StartupGrant::new("hexcasting:give_staff")
            .item("hexcasting:staff")
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
        if phase == EventPhase::Post && ev.item_id == "hexcasting:staff" {
            if ev.sneaking {
                vm::reset(&ev.player_name, srv);
                srv.send_to_player(&ev.player_name, grid::CH_OPEN, b"reset");
            } else {
                srv.send_to_player(&ev.player_name, grid::CH_OPEN, b"");
            }
        }
        true
    });

    // Server: a drawn pattern arrives — run it through the casting VM.
    registry.on_packet(grid::CH_CAST, |ev, srv| {
        vm::handle_cast(&ev.player, &ev.payload, srv);
    });

    // Client: open-grid order from the server ("reset" wipes the session).
    registry.on_client_packet(grid::CH_OPEN, |ev, _srv| {
        if ev.payload == b"reset" {
            grid::on_reset();
        }
        grid::on_open();
        yog_api::open_ui(grid::UI_ID, true, false);
    });

    // Client: refreshed stack view for the grid side panel.
    registry.on_client_packet(grid::CH_STACK, |ev, _srv| {
        grid::on_stack_packet(&ev.payload);
    });
}
