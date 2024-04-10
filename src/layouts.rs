binary_layout::binary_layout!(rdm_request_layout, BigEndian, {
    start_code: u8,
    sub_start_code: u8,
    message_length: u8,
    destination_uid: [u8; 6],
    source_uid: [u8; 6],
    transaction_number: u8,
    port_id_response_type: u8,
    message_count: u8,
    sub_device: u16,
    command_class: u8,
    parameter_id: u16,
    parameter_data_length: u8,
    parameter_data_and_checksum: [u8],
});

binary_layout::binary_layout!(rdm_status_message_layout, BigEndian, {
    sub_device_id: u16,
    status_type: u8,
    status_message_id: u16,
    data_value_1: u16,
    data_value_2: u16,
});

binary_layout::binary_layout!(rdm_device_info_layout, BigEndian, {
    protocol_version: u16,
    device_model_id: u16,
    product_category: u16,
    software_version_id: u32,
    dmx_footprint: u16,
    dmx_personality: u16,
    dmx_start_address: u16,
    sub_device_count: u16,
    sensor_count: u8,
});
