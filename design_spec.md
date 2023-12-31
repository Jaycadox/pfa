# Packed File Archive (.pfa)

## The format
All numbers are represented in Little Endian.

pfa{header}{catalog}{data}

### header
{version:u8}{archive_name_size:u8}{archive_name:u8\[archive_name_size\]}{extra_data_len:u64}{extra_data:u8\[extra_data_len\]}

### catalog
{num_entries:u64}{entries:entry\[num_entries\]}

### entry
{path_name:u8\[32\]}{slice:catalog_slice|data_slice}

Entries which are directories contain a catalog_slice, while entires which are files contain a data slice, the sizes of both structs are the same.

path_name is null terminated.

#### catalog_slice
{slice_flags}{size:u64}{offset:u64}

entry_offset is the offset of the entry in the catalog from the current entry.

size dictates how many catalog entries (starting at the idx) are inside of the directory

#### data_slice
{slice_flags}{size:u64}{offset:u64}

data_offset is the number of bytes from the start of the raw data, and size is the number of bytes which should be read from that location.

#### slice_flags
{use_compression:u1}{password_encrypted:u1}{error_correction:u1}{reserved:u5}

Bitfield with a size of 1 byte

Note: use_compression is only supported for data slices, not catalog slices. The format for compression is LZ4 with a little endian u32 size prepended.
Note: password_encrypted is only supported for data slices, it uses an AES256-GCM cipher.
Note: error_correction is only supported for data slices, it uses Reed Solomon encoding.

### data
{data_size:u64}{data:u8\[data_size\]}
