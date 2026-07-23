#[cfg(test)]
mod tests {
    use crate::{build_pc, HeapBuilder, PropertyValue};

    #[test]
    fn test_heap_large() {
        let mut heap = HeapBuilder::new(0x6C);
        let props = vec![
            (0x0037, PropertyValue::String("Impressive Deals for Impressive Mother".to_string())),
            (0x0C1F, PropertyValue::String("\"CVS Photo\" <photo@mystore.cvs.com>".to_string())),
            (0x1035, PropertyValue::String("<38b4d0af-d219-4d3b-ae65-b96ae6451545@dfw1s10mta14.local>".to_string())),
            (0x0E08, PropertyValue::I32(1024)),
            (0x0E1B, PropertyValue::Bool(false)),
            (0x0039, PropertyValue::Time(0x01D5B035EDA780_i64)),
            (0x1000, PropertyValue::String("This is a multi-part message in MIME format.\n\n--m5FAU7X6UZMA=_?:\nContent-Type: text/plain;\n\tcharset=\"utf-8\"\nContent-Transfer-Encoding: 8bit\n\n\n\n \nCVS\n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n \n".to_string())),
        ];
        let hid = build_pc(&mut heap, &props);
        let data = heap.finalize(hid);

        println!("Large heap size: {}", data.len());
        assert!(data.len() < 1000, "heap should be under 1000 bytes");

        let ib_hnpm = u16::from_le_bytes([data[0], data[1]]);
        println!("ibHnpm={}", ib_hnpm);

        let c_alloc = u16::from_le_bytes([data[ib_hnpm as usize], data[ib_hnpm as usize + 1]]);
        println!("cAlloc={}", c_alloc);

        assert!(c_alloc > 0 && c_alloc < 100, "cAlloc should be reasonable");

        // HNPAGEMAP = cAlloc(2) + cFree(2) + rgibAlloc[...] — skip both fields.
        let rgib_start = ib_hnpm as usize + 4;
        let expected_c_alloc = 7; // 4 strings + time + BTH header + BTH leaf records
        assert_eq!(c_alloc, expected_c_alloc, "wrong number of allocations");

        for i in 0..=c_alloc {
            let off = rgib_start + i as usize * 2;
            if off + 2 <= data.len() {
                let val = u16::from_le_bytes([data[off], data[off + 1]]);
                println!("  rgibAlloc[{}] = {}", i, val);
                assert!(
                    (val as usize) <= data.len(),
                    "rgibAlloc[{}] = {} exceeds data.len() = {}",
                    i,
                    val,
                    data.len()
                );
            }
        }

        // Verify hidUserRoot points to BTH header allocation
        let hid_user_root = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let alloc_index = (hid_user_root >> 5) as u16;
        println!(
            "hidUserRoot=0x{:X}, alloc_index={}",
            hid_user_root, alloc_index
        );
        assert!(
            alloc_index > 0 && alloc_index <= c_alloc,
            "BTH header alloc index out of range"
        );

        // Read BTH header
        let offset_a = rgib_start + (alloc_index as usize - 1) * 2;
        let offset_b = rgib_start + alloc_index as usize * 2;
        let bth_start = u16::from_le_bytes([data[offset_a], data[offset_a + 1]]) as usize;
        let bth_end = u16::from_le_bytes([data[offset_b], data[offset_b + 1]]) as usize;
        println!("BTH header at {}..{}", bth_start, bth_end);
        assert_eq!(bth_end - bth_start, 8, "BTH header should be 8 bytes");
        assert_eq!(data[bth_start], 0xB5, "BTH signature mismatch");
    }
}
