//! Host tests for the siop microcode: the entry points and patch sites stay inside their
//! tables, and a reference test compares every table and define with `siop.out`.

use std::vec::Vec;

use super::*;
use crate::crypto::testutil::c_table;
use crate::reftest;

/// The microcode file inside the reference tree.
const SIOP_OUT: &str = "sys/dev/microcode/siop/siop.out";

#[test]
fn entry_points_and_patch_sites_are_inside_their_tables() {
    let script = siop_script.len() as u32 * 4;
    for ent in [
        Ent_waitphase,
        Ent_send_msgout,
        Ent_msgin,
        Ent_msgin_ack,
        Ent_reselect,
        Ent_script_sched,
        Ent_script_sched_slot0,
        Ent_get_extmsgdata,
        Ent_resel_targ0,
        Ent_msgin_space,
        Ent_lunsw_return,
        Ent_led_on1,
        Ent_led_on2,
        Ent_led_off,
    ] {
        assert!(ent < script && ent % 4 == 0, "{ent:#x}");
    }
    // 40 scheduler slots of two words each, all free (`JUMP foo, IF FALSE`).
    for slot in 0..40 {
        assert_eq!(
            siop_script[(Ent_script_sched_slot0 / 4) as usize + slot * 2],
            0x8000_0000
        );
    }
    assert!(
        E_abs_msgin_Used
            .iter()
            .all(|&w| w < siop_script.len() as u32)
    );
    assert!(Ent_lun_switch_entry / 4 < lun_switch.len() as u32);
    assert!((E_abs_lunsw_return_Used[0] as usize) < lun_switch.len());
    assert!(((Ent_resel_tag0 / 4) as usize + 15 * 2) < tag_switch.len());
    for used in [
        E_ldsa_abs_reselected_Used[0],
        E_ldsa_abs_reselect_Used[0],
        E_ldsa_abs_selected_Used[0],
        E_ldsa_abs_data_Used[0],
        E_ldsa_abs_slot_Used[0],
    ] {
        assert!((used as usize) < load_dsa.len());
    }
    // The register loads siop_morecbd patches are `MOVE data8 TO DSA0..3`.
    for (ent, reg) in [
        (Ent_rdsa0, 0x10),
        (Ent_rdsa1, 0x11),
        (Ent_rdsa2, 0x12),
        (Ent_rdsa3, 0x13),
    ] {
        assert_eq!(load_dsa[(ent / 4) as usize] >> 16, 0x7800 | reg);
    }
}

#[test]
#[ignore = "reads the C reference (just test-ref)"]
fn tables_and_defines_match_siop_out() {
    let tables: &[(&str, &[u32])] = &[
        ("siop_script", &siop_script),
        ("lun_switch", &lun_switch),
        ("tag_switch", &tag_switch),
        ("load_dsa", &load_dsa),
        ("siop_led_on", &siop_led_on),
        ("siop_led_off", &siop_led_off),
        (
            "E_abs_script_sched_slot0_Used",
            &E_abs_script_sched_slot0_Used,
        ),
        ("E_abs_targ0_Used", &E_abs_targ0_Used),
        ("E_abs_msgin_Used", &E_abs_msgin_Used),
        ("E_abs_lunsw_return_Used", &E_abs_lunsw_return_Used),
        ("E_abs_tag0_Used", &E_abs_tag0_Used),
        ("E_ldsa_abs_reselected_Used", &E_ldsa_abs_reselected_Used),
        ("E_ldsa_abs_reselect_Used", &E_ldsa_abs_reselect_Used),
        ("E_ldsa_abs_selected_Used", &E_ldsa_abs_selected_Used),
        ("E_ldsa_abs_data_Used", &E_ldsa_abs_data_Used),
        ("E_ldsa_abs_slot_Used", &E_ldsa_abs_slot_Used),
    ];
    for &(name, ours) in tables {
        let theirs: Vec<u64> = c_table(SIOP_OUT, name);
        let ours: Vec<u64> = ours.iter().map(|&w| u64::from(w)).collect();
        assert_eq!(ours, theirs, "{name}");
    }

    let defs = reftest::defines(SIOP_OUT);
    let ours: &[(&str, u32)] = &[
        ("A_t_id", A_t_id),
        ("A_t_msg_in", A_t_msg_in),
        ("A_t_ext_msg_in", A_t_ext_msg_in),
        ("A_t_ext_msg_data", A_t_ext_msg_data),
        ("A_t_msg_out", A_t_msg_out),
        ("A_t_cmd", A_t_cmd),
        ("A_t_status", A_t_status),
        ("A_t_data", A_t_data),
        ("A_int_done", A_int_done),
        ("A_int_msgin", A_int_msgin),
        ("A_int_extmsgin", A_int_extmsgin),
        ("A_int_extmsgdata", A_int_extmsgdata),
        ("A_int_disc", A_int_disc),
        ("A_int_saveoffset", A_int_saveoffset),
        ("A_int_reseltarg", A_int_reseltarg),
        ("A_int_resellun", A_int_resellun),
        ("A_int_reseltag", A_int_reseltag),
        ("A_int_resfail", A_int_resfail),
        ("A_int_err", A_int_err),
        ("A_flag_sdp", A_flag_sdp),
        ("A_flag_data", A_flag_data),
        ("A_flag_data_mask", A_flag_data_mask),
        ("Ent_waitphase", Ent_waitphase),
        ("Ent_send_msgout", Ent_send_msgout),
        ("Ent_msgout", Ent_msgout),
        ("Ent_msgin", Ent_msgin),
        ("Ent_handle_msgin", Ent_handle_msgin),
        ("Ent_msgin_ack", Ent_msgin_ack),
        ("Ent_dataout", Ent_dataout),
        ("Ent_datain", Ent_datain),
        ("Ent_cmdout", Ent_cmdout),
        ("Ent_status", Ent_status),
        ("Ent_disconnect", Ent_disconnect),
        ("Ent_reselect", Ent_reselect),
        ("Ent_reselected", Ent_reselected),
        ("Ent_selected", Ent_selected),
        ("Ent_script_sched", Ent_script_sched),
        ("Ent_script_sched_slot0", Ent_script_sched_slot0),
        ("Ent_get_extmsgdata", Ent_get_extmsgdata),
        ("Ent_resel_targ0", Ent_resel_targ0),
        ("Ent_msgin_space", Ent_msgin_space),
        ("Ent_lunsw_return", Ent_lunsw_return),
        ("Ent_led_on1", Ent_led_on1),
        ("Ent_led_on2", Ent_led_on2),
        ("Ent_led_off", Ent_led_off),
        ("E_abs_script_sched_slot0", E_abs_script_sched_slot0),
        ("E_abs_targ0", E_abs_targ0),
        ("E_abs_msgin", E_abs_msgin),
        ("Ent_lun_switch_entry", Ent_lun_switch_entry),
        ("Ent_resel_lun0", Ent_resel_lun0),
        ("Ent_restore_scntl3", Ent_restore_scntl3),
        ("E_abs_lunsw_return", E_abs_lunsw_return),
        ("Ent_tag_switch_entry", Ent_tag_switch_entry),
        ("Ent_resel_tag0", Ent_resel_tag0),
        ("E_abs_tag0", E_abs_tag0),
        ("Ent_rdsa0", Ent_rdsa0),
        ("Ent_rdsa1", Ent_rdsa1),
        ("Ent_rdsa2", Ent_rdsa2),
        ("Ent_rdsa3", Ent_rdsa3),
        ("Ent_ldsa_reload_dsa", Ent_ldsa_reload_dsa),
        ("Ent_ldsa_select", Ent_ldsa_select),
        ("Ent_ldsa_data", Ent_ldsa_data),
        ("E_ldsa_abs_reselected", E_ldsa_abs_reselected),
        ("E_ldsa_abs_reselect", E_ldsa_abs_reselect),
        ("E_ldsa_abs_selected", E_ldsa_abs_selected),
        ("E_ldsa_abs_data", E_ldsa_abs_data),
        ("E_ldsa_abs_slot", E_ldsa_abs_slot),
    ];
    // Every define of siop.out is listed above.
    assert_eq!(defs.len(), ours.len());
    for &(name, v) in ours {
        assert_eq!(reftest::int(&defs, name), Some(i64::from(v)), "{name}");
    }
}
