#!/usr/bin/env python3
"""Embed a .ico file into a Windows PE executable by adding a .rsrc section.

This manually constructs a PE resource directory with RT_ICON entries,
adds a .rsrc section, and updates all headers correctly.
"""
import pefile
import struct
import sys


def align(val, alignment):
    return (val + alignment - 1) & ~(alignment - 1)


def ico_to_images(ico_data):
    """Parse an .ico file into a list of image dicts."""
    if len(ico_data) < 6:
        raise ValueError("ICO file too small")
    reserved = struct.unpack_from("<H", ico_data, 0)[0]
    image_type = struct.unpack_from("<H", ico_data, 2)[0]
    count = struct.unpack_from("<H", ico_data, 4)[0]
    if reserved != 0 or image_type != 1:
        raise ValueError("Not a valid ICO file")

    images = []
    offset = 6
    for _ in range(count):
        if offset + 16 > len(ico_data):
            break
        width = ico_data[offset]
        height = ico_data[offset + 1]
        bit_count = struct.unpack_from("<H", ico_data, offset + 6)[0]
        size_in_bytes = struct.unpack_from("<I", ico_data, offset + 8)[0]
        offset_to_data = struct.unpack_from("<I", ico_data, offset + 12)[0]

        image_data = ico_data[offset_to_data : offset_to_data + size_in_bytes]
        images.append(
            {
                "width": width if width != 0 else 256,
                "height": height if height != 0 else 256,
                "bit_count": bit_count,
                "data": image_data,
            }
        )
        offset += 16
    return images


def embed_icon(exe_path, ico_path):
    images = ico_to_images(open(ico_path, "rb").read())
    if not images:
        print(f"No images found in {ico_path}", file=sys.stderr)
        sys.exit(1)

    pe = pefile.PE(exe_path)
    pe_data = bytearray(pe.__data__)

    file_alignment = pe.OPTIONAL_HEADER.FileAlignment
    section_alignment = pe.OPTIONAL_HEADER.SectionAlignment

    # --- Parse key headers ---
    e_lfanew = struct.unpack_from("<I", pe_data, 0x3C)[0]
    coff_header_start = e_lfanew + 4  # after "PE\0\0"
    opt_header_start = e_lfanew + 24  # after COFF header (20 bytes)
    opt_magic = struct.unpack_from("<H", pe_data, opt_header_start)[0]
    is_pe32_plus = opt_magic == 0x20B

    # Number of sections (COFF header offset +2)
    num_sections = struct.unpack_from("<H", pe_data, coff_header_start + 2)[0]

    # Optional Header size (from COFF header offset +16)
    opt_header_size = struct.unpack_from("<H", pe_data, coff_header_start + 16)[0]

    # Section table starts right after Optional Header
    section_table_start = opt_header_start + opt_header_size

    # Last section info
    last_section = pe.sections[-1]
    last_vaddr = last_section.VirtualAddress
    last_vsize = align(last_section.Misc_VirtualSize, section_alignment)
    last_raw_ptr = last_section.PointerToRawData
    last_raw_size = last_section.SizeOfRawData

    # New section location
    new_vaddr = align(last_vaddr + last_vsize, section_alignment)
    new_raw_ptr = align(last_raw_ptr + last_raw_size, file_alignment)

    # --- Build resource directory and icon data ---
    RT_ICON = 3
    RT_GROUP_ICON = 14
    LANG_EN_US = 0x0409
    SUBLANG_DEFAULT = 0x01
    LANG_ID = LANG_EN_US | SUBLANG_DEFAULT

    # Build icon directory (RT_GROUP_ICON) - references individual RT_ICON images
    # GRPICONDIR: idReserved(2) + idType(2) + idCount(2) = 6 bytes
    # GRPICONDIRENTRY per image: bWidth(1) + bHeight(1) + bColorCount(1) + bReserved(1) +
    #                            wPlanes(2) + wBitCount(2) + dwBytesInRes(4) +
    #                            dwImageOffset(4) + nID(2) = 14 bytes
    grp_icon_dir = struct.pack("<HHH", 0, 1, len(images))  # reserved, type=1, count
    grp_entries = b""
    for idx, img in enumerate(images):
        entry = struct.pack(
            "<BBBBHHIIH",
            img["width"] if img["width"] < 256 else 0,
            img["height"] if img["height"] < 256 else 0,
            0, 0,  # color_count, reserved
            1, img["bit_count"],  # planes, bit_count
            len(img["data"]),  # bytes_in_res
            0,  # image_offset (filled later)
            idx + 1,  # resource ID
        )
        grp_entries += entry

    # Build RT_ICON data (individual images, each preceded by its ICONDIRENTRY)
    # Actually, RT_ICON resource data is just the image data (BMP/DIB header + pixels),
    # NOT including an ICONDIRENTRY. The ICONDIRENTRY is only in the group.
    # Wait, no. Let me check the Windows format:
    # - RT_GROUP_ICON: GRPICONDIR + GRPICONDIRENTRYs (references to RT_ICON resources)
    # - RT_ICON: The image data itself, which is an ICONIMAGE (BITMAPINFOHEADER + color table + XOR mask + AND mask)
    #
    # But an .ico file contains: ICONDIR + ICONDIRENTRYs + image data blocks
    # Each image data block starts with a BITMAPINFOHEADER (which is what RT_ICON expects)
    
    # So RT_ICON resource data = the image data block from the .ico file (starting at offset_to_data)
    # And RT_GROUP_ICON = ICONDIR + modified ICONDIRENTRYs (where dwImageOffset is the RT_ICON resource ID)
    
    # Actually wait, I need to double-check. In an .ico file:
    # - ICONDIR: idReserved, idType, idCount
    # - ICONDIRENTRY: bWidth, bHeight, bColorCount, bReserved, wPlanes, wBitCount, dwBytesInRes, dwImageOffset, nID
    # - Image data: starts at dwImageOffset, contains BITMAPINFOHEADER + ...
    
    # For RT_GROUP_ICON resource:
    # Same as ICONDIR + ICONDIRENTRYs, but dwImageOffset field is replaced by the RT_ICON resource ID
    
    # For RT_ICON resource:
    # The raw image data (BITMAPINFOHEADER + ...) - same as what's in the .ico file at dwImageOffset
    
    # Build RT_ICON data (concatenation of all image data blocks)
    rt_icon_data = b""
    image_offsets = []  # offset of each image within rt_icon_data
    for img in images:
        image_offsets.append(len(rt_icon_data))
        rt_icon_data += img["data"]

    # Now fix up the group entries with correct resource IDs
    # Each image in RT_GROUP_ICON references an RT_ICON resource by ID (idx+1)
    # dwImageOffset in GRPICONDIRENTRY is replaced by the resource ID
    grp_entries = b""
    for idx, img in enumerate(images):
        entry = struct.pack(
            "<BBBBHHIIH",
            img["width"] if img["width"] < 256 else 0,
            img["height"] if img["height"] < 256 else 0,
            0, 0,
            1, img["bit_count"],
            len(img["data"]),
            idx + 1,  # nID references RT_ICON resource ID
            idx + 1,  # dwImageOffset replaced with resource ID
        )
        grp_entries += entry

    grp_icon_data = grp_icon_dir + grp_entries

    # --- Calculate resource directory layout ---
    # We need:
    # RT_GROUP_ICON (type 14): 1 resource (ID 1) -> grp_icon_data
    # RT_ICON (type 3): N resources (ID 1..N) -> each image data

    RES_DIR_ENTRY = 16
    RES_DIR_TABLE_HEADER = 16
    RES_DATA_ENTRY = 16

    num_rt_icon_entries = len(images)
    num_rt_group_icon_entries = 1

    # Size of each language directory table:
    # RT_ICON: header(16) + N entries(16*N) = 16 + 16*N
    # RT_GROUP_ICON: header(16) + 1 entry(16) = 32
    rt_icon_lang_size = 16 + 16 * num_rt_icon_entries
    rt_group_icon_lang_size = 32

    # Name directory table for each type: header(16) + 1 entry(16) = 32
    # Type directory table: header(16) + 2 entries(32) = 48
    # Root directory table: header(16) + 2 entries(32) = 48

    # Layout (all offsets relative to start of resource data):
    # [Root Dir Table (48)] [Type Dir Table (48)] [Name Dir Table RT_GROUP_ICON (32)] [Lang Dir Table RT_GROUP_ICON (32)] [Data Entries RT_GROUP_ICON (16)] [Name Dir Table RT_ICON (32)] [Lang Dir Table RT_ICON (16+16*N)] [Data Entries RT_ICON (16*N)] [RT_GROUP_ICON data] [RT_ICON data]

    root_size = 48
    type_size = 48
    name_size_per_type = 32
    rt_group_icon_lang_size = 32
    rt_icon_lang_size = 16 + 16 * num_rt_icon_entries
    rt_group_icon_data_size = len(grp_icon_data)
    rt_icon_data_size = len(rt_icon_data)

    # Offsets relative to resource directory start
    root_offset = 0
    type_offset = root_offset + root_size
    name_grp_offset = type_offset + type_size
    lang_grp_offset = name_grp_offset + name_size_per_type
    data_grp_offset = lang_grp_offset + rt_group_icon_lang_size
    name_icon_offset = data_grp_offset + RES_DATA_ENTRY
    lang_icon_offset = name_icon_offset + name_size_per_type
    data_icon_offset = lang_icon_offset + rt_icon_lang_size
    grp_icon_file_offset = data_icon_offset + RES_DATA_ENTRY * num_rt_icon_entries
    icon_data_file_offset = grp_icon_file_offset + rt_group_icon_data_size

    total_rsrc_size = icon_data_file_offset + rt_icon_data_size

    # Align to section alignment
    new_vsize = align(total_rsrc_size, section_alignment)
    new_raw_size = align(total_rsrc_size, file_alignment)

    # Extend PE data with new section
    new_file_size = new_raw_ptr + new_raw_size
    pe_data = pe_data + b"\x00" * (new_file_size - len(pe_data))

    # --- Write resource directory into the new section area ---
    base = new_raw_ptr  # file offset where .rsrc section starts

    def pack_at(file_offset, fmt, *values):
        struct.pack_into(fmt, pe_data, base + file_offset, *values)

    # 5. Root Directory Table
    pack_at(root_offset, "<IIHHHH", 0, 0, 1, 1, 0, 2)  # 2 id entries (RT_GROUP_ICON, RT_ICON)
    # Entry 0: RT_GROUP_ICON
    pack_at(root_offset + 16, "<II", RT_GROUP_ICON | 0x80000000, type_offset - root_offset)
    # Entry 1: RT_ICON
    pack_at(root_offset + 32, "<II", RT_ICON | 0x80000000, type_offset - root_offset)

    # 6. Type Directory Table
    pack_at(type_offset, "<IIHHHH", 0, 0, 1, 1, 0, 1)  # 1 id entry
    # Entry: RT_GROUP_ICON -> Name Directory
    pack_at(type_offset + 16, "<II", RT_GROUP_ICON | 0x80000000, name_grp_offset - type_offset)
    # Wait, we have 2 types but only 1 entry? Let me fix.
    # Actually, root says 2 id entries, but type table says 1? That's wrong.
    # Let me restructure: root should point to type table which has 2 entries.

    # Fix: Type directory has 2 entries (RT_GROUP_ICON and RT_ICON)
    pack_at(type_offset, "<IIHHHH", 0, 0, 1, 1, 0, 2)
    # Entry 0: RT_GROUP_ICON
    pack_at(type_offset + 16, "<II", RT_GROUP_ICON | 0x80000000, name_grp_offset - type_offset)
    # Entry 1: RT_ICON
    pack_at(type_offset + 32, "<II", RT_ICON | 0x80000000, name_icon_offset - type_offset)

    # 7. Name Directory for RT_GROUP_ICON
    pack_at(name_grp_offset, "<IIHHHH", 0, 0, 1, 1, 0, 1)  # 1 id entry (ID=1)
    pack_at(name_grp_offset + 16, "<II", 1 | 0x80000000, lang_grp_offset - name_grp_offset)

    # 8. Language Directory for RT_GROUP_ICON
    pack_at(lang_grp_offset, "<IIHHHH", 0, 0, 1, 1, 0, 1)  # 1 id entry
    pack_at(lang_grp_offset + 16, "<II", LANG_ID, data_grp_offset - lang_grp_offset)

    # 9. Data Entry for RT_GROUP_ICON
    pack_at(data_grp_offset, "<IIII", new_vaddr + grp_icon_file_offset, rt_group_icon_data_size, 0, 0)

    # 10. Name Directory for RT_ICON
    pack_at(name_icon_offset, "<IIHHHH", 0, 0, 1, 1, 0, num_rt_icon_entries)
    for idx in range(num_rt_icon_entries):
        pack_at(name_icon_offset + 16 + idx * RES_DIR_ENTRY, "<II",
                (idx + 1) | 0x80000000, lang_icon_offset - name_icon_offset)

    # 11. Language Directory for RT_ICON
    pack_at(lang_icon_offset, "<IIHHHH", 0, 0, 1, 1, 0, num_rt_icon_entries)
    for idx in range(num_rt_icon_entries):
        pack_at(lang_icon_offset + 16 + idx * RES_DIR_ENTRY, "<II", LANG_ID,
                data_icon_offset - lang_icon_offset + idx * RES_DATA_ENTRY)

    # 12. Data Entries for RT_ICON
    for idx in range(num_rt_icon_entries):
        pack_at(data_icon_offset + idx * RES_DATA_ENTRY, "<IIII",
                new_vaddr + icon_data_file_offset + image_offsets[idx],
                len(images[idx]["data"]), 0, 0)

    # 13. Resource data
    pe_data[base + grp_icon_file_offset:base + grp_icon_file_offset + rt_group_icon_data_size] = grp_icon_data
    pe_data[base + icon_data_file_offset:base + icon_data_file_offset + rt_icon_data_size] = rt_icon_data

    # --- Add .rsrc section header ---
    new_section_offset = section_table_start + num_sections * 40
    struct.pack_into("<8sIIIIIIHHI", pe_data, new_section_offset,
        b".rsrc\x00\x00\x00",
        new_vsize,
        new_vaddr,
        new_raw_size,
        new_raw_ptr,
        0, 0, 0, 0,
        0x40000040,  # CNT_INITIALIZED_DATA | MEM_READ
    )

    # --- Update COFF header: NumberOfSections ---
    struct.pack_into("<H", pe_data, coff_header_start + 2, num_sections + 1)

    # --- Update Optional Header: SizeOfImage ---
    # SizeOfImage offset in Optional Header:
    # PE32: offset 56 from Optional Header start
    # PE32+: offset 56 from Optional Header start (same)
    new_size_of_image = align(new_vaddr + new_vsize, section_alignment)
    struct.pack_into("<I", pe_data, opt_header_start + 56, new_size_of_image)

    # --- Update Data Directory for resources ---
    # Data Directory starts after Optional Header standard fields
    # PE32: 96 bytes before data dirs
    # PE32+: 112 bytes before data dirs
    opt_dir_start = opt_header_start + (112 if is_pe32_plus else 96)
    # Entry 2 = IMAGE_DIRECTORY_ENTRY_RESOURCE
    data_dir_entry_offset = opt_dir_start + 2 * 8
    struct.pack_into("<II", pe_data, data_dir_entry_offset, new_vaddr, total_rsrc_size)

    with open(exe_path, "r+b") as f:
        f.write(pe_data)

    print(f"Embedded {len(images)} icon(s) into {exe_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <exe_path> <ico_path>", file=sys.stderr)
        sys.exit(1)
    embed_icon(sys.argv[1], sys.argv[2])
