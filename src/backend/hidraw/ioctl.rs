//! The IOCTL calls we need for the native linux backend

use nix::{ioctl_read, ioctl_readwrite_buf};

// From linux/hidraw.h
const HIDRAW_IOC_MAGIC: u8 = b'H';
const HIDRAW_IOC_GRDESCSIZE: u8 = 0x01;
// const HIDRAW_IOC_SFEATURE: u8 = 0x06;
const HIDRAW_IOC_GFEATURE: u8 = 0x07;
const HIDRAW_IOC_GINPUT: u8 = 0x0A;

ioctl_read!(hidraw_ioc_grdescsize, HIDRAW_IOC_MAGIC, HIDRAW_IOC_GRDESCSIZE, i32);
// ioctl_readwrite_buf!(hidraw_ioc_set_feature, HIDRAW_IOC_MAGIC, HIDRAW_IOC_SFEATURE, u8);
ioctl_readwrite_buf!(hidraw_ioc_get_feature, HIDRAW_IOC_MAGIC, HIDRAW_IOC_GFEATURE, u8);
ioctl_readwrite_buf!(hidraw_ioc_ginput, HIDRAW_IOC_MAGIC, HIDRAW_IOC_GINPUT, u8);