#!/usr/bin/env python3
"""Embed a .ico file into a Windows PE executable as its application icon.

Uses pefile to add RT_ICON resources to the .exe after compilation.
"""
import pefile
import struct
import sys


def align(val, alignment):
    return (val + alignment - 1) & ~(alignment - 1)


def ico_to_images(ico_data):
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

    # Build full icon directory: header + entries + image data
    num_images = len(images)
    header = struct.pack("<HHH", 0, 1, num_images)
    dir_entries = b""
    image_blocks = b""
    current_offset = 6 + num_images * 16
    for img in images:
        entry = struct.pack(
            "<BBBBHHII",
            img["width"] if img["width"] < 256 else 0,
            img["height"] if img["height"] < 256 else 0,
            0, 0, 1, img["bit_count"], len(img["data"]), current_offset,
        )
        dir_entries += entry
        image_blocks += img["data"]
        current_offset += len(img["data"])
    full_icon_data = header + dir_entries + image_blocks
    icon_data_size = len(full_icon_data)

    # Find append location
    file_size = len(pe_data)
    data_file_offset = align(file_size, file_alignment)
    max_vaddr = 0
    max_raw_end = 0
    for section in pe.sections:
        vaddr_end = section.VirtualAddress + align(section.Misc_VirtualSize, section_alignment)
        raw_end = section.PointerToRawData + section.SizeOfRawData
        max_vaddr = max(max_vaddr, vaddr_end)
        max_raw_end = max(max_raw_end, raw_end)
    data_vaddr = align(max_vaddr, section_alignment)
    data_ptr = align(max_raw_end, file_alignment)

    # Extend file with icon image data
    new_size = data_ptr + icon_data_size
    pe_data = pe_data + b"\x00" * (new_size - len(pe_data))
    pe_data = (
        pe_data[:data_ptr] + full_icon_data + pe_data[data_ptr + icon_data_size:]
    )

    # Build resource directory
    num_images = len(images)
    root_size = 32
    type_size = 32
    name_size = 32
    lang_size = 16 + 16 * num_images
    data_entries_size = 16 * num_images
    dir_total = root_size + type_size + name_size + lang_size + data_entries_size

    res_dir_file_offset = align(data_ptr + icon_data_size, file_alignment)
    res_dir_vaddr = align(data_vaddr + icon_data_size, section_alignment)
    final_size = res_dir_file_offset + dir_total
    pe_data = pe_data + b"\x00" * (final_size - len(pe_data))

    LANG_EN_US = 0x0409
    RT_ICON = 11
    RES_DIR_ENTRY = 16

    # 1. Data entries
    rde_base = res_dir_file_offset + root_size + type_size + name_size + lang_size
    img_offset_in_block = 6 + num_images * 16
    for idx, img in enumerate(images):
        entry_offset = rde_base + idx * RES_DIR_ENTRY
        img_rva = data_vaddr + img_offset_in_block
        struct.pack_into(
            "<IIII", pe_data, entry_offset,
            img_rva, len(img["data"]), 0, 0,
        )
        img_offset_in_block += len(img["data"])

    # 2. Language directory: header + one entry per image
    lang_base = rde_base - lang_size
    # Header
    struct.pack_into("<IIHHHH", pe_data, lang_base, 0, 0, 1, 1, 0, num_images)
    for idx in range(num_images):
        entry_offset = lang_base + 16 + idx * RES_DIR_ENTRY
        # Language ID (DWORD), Offset to Resource Data Entry (DWORD)
        struct.pack_into(
            "<II", pe_data, entry_offset,
            LANG_EN_US,  # Language ID
            (rde_base - res_dir_file_offset) + idx * RES_DIR_ENTRY,
        )

    # 3. Name directory: header + one entry pointing to lang directory
    name_base = lang_base - name_size
    struct.pack_into("<IIHHHH", pe_data, name_base, 0, 0, 1, 1, 0, 1)
    entry_offset = name_base + 16
    struct.pack_into(
        "<II", pe_data, entry_offset,
        1 | 0x80000000,  # ID=1, directory flag (bit 31)
        lang_base - res_dir_file_offset,
    )

    # 4. Type directory: header + one entry pointing to name directory
    type_base = name_base - type_size
    struct.pack_into("<IIHHHH", pe_data, type_base, 0, 0, 1, 1, 0, 1)
    entry_offset = type_base + 16
    struct.pack_into(
        "<II", pe_data, entry_offset,
        RT_ICON | 0x80000000,  # Type 11, directory flag
        name_base - res_dir_file_offset,
    )

    # 5. Root directory: header + one entry pointing to type directory
    root_base = res_dir_file_offset
    struct.pack_into("<IIHHHH", pe_data, root_base, 0, 0, 1, 1, 0, 1)
    entry_offset = root_base + 16
    struct.pack_into(
        "<II", pe_data, entry_offset,
        RT_ICON | 0x80000000,  # Type 11, directory flag
        type_base - res_dir_file_offset,
    )

    # Update Optional Header data directory for resources (index 2)
    e_lfanew = struct.unpack_from("<I", pe_data, 0x3C)[0]
    opt_header_start = e_lfanew + 24
    opt_magic = struct.unpack_from("<H", pe_data, opt_header_start)[0]
    opt_header_size_before_dirs = 112 if opt_magic == 0x20b else 96
    data_dir_offset = opt_header_start + opt_header_size_before_dirs + 2 * 8
    struct.pack_into("<II", pe_data, data_dir_offset, res_dir_vaddr, dir_total)

    with open(exe_path, "r+b") as f:
        f.write(pe_data)

    print(f"Embedded {num_images} icon(s) into {exe_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <exe_path> <ico_path>", file=sys.stderr)
        sys.exit(1)
    embed_icon(sys.argv[1], sys.argv[2])
