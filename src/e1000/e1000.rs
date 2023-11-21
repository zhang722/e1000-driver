// e1000 Driver for Intel 82540EP/EM
use super::e1000_const::*;
use super::super::Ext;
use super::super::Volatile;
use alloc::vec::Vec;
use core::{cmp::min, mem::size_of, slice::from_raw_parts_mut};
use crate::utils::*;

const TX_RING_SIZE: usize = 256;
const RX_RING_SIZE: usize = 256;
const MBUF_SIZE: usize = 2048;

/// Kernel functions that drivers must use
pub trait KernelFunc {
    /// Page size (usually 4K)
    const PAGE_SIZE: usize = 4096;

    // 或请求分配irq

    /// Allocate consequent physical memory for DMA;
    /// Return (cpu virtual address, dma physical address) which is page aligned.
    //fn dma_alloc_coherent(pages: usize) -> usize;
    fn dma_alloc_coherent(&mut self, pages: usize) -> (usize, usize);

    /// Deallocate DMA memory by virtual address
    fn dma_free_coherent(&mut self, vaddr: usize, pages: usize);
}

/// Main structure of the e1000 driver.
/// Used to save members such as ring buffer.
pub struct E1000Device<'a, K: KernelFunc> {
    regs: &'static mut [Volatile<u32>],
    rx_ring_dma: usize,
    tx_ring_dma: usize,
    rx_ring: &'a mut [RxDesc], //可以只为ring buffer加锁
    tx_ring: &'a mut [TxDesc],
    rx_mbufs: Vec<usize>,
    tx_mbufs: Vec<usize>,
    mbuf_size: usize,
    //phy_interface: PhyInterfaceMode,
    kfn: K,
}

// struct spinlock e1000_lock;

/// [E1000 3.3.3]
/// The dma descriptor for transmitting
#[derive(Debug, Clone)]
#[repr(C, align(16))]
pub struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

/// [E1000 3.2.3]
/// The dma descriptor for receiving
#[derive(Debug, Clone)]
#[repr(C, align(16))]
pub struct RxDesc {
    addr: u64,   /* Address of the descriptor's data buffer */
    length: u16, /* Length of data DMAed into data buffer */
    csum: u16,   /* Packet checksum */
    status: u8,  /* Descriptor status */
    errors: u8,  /* Descriptor Errors */
    special: u16,
}

impl<'a, K: KernelFunc> E1000Device<'a, K> {
    /// New an e1000 device by Allocating memory
    pub fn new(mut kfn: K, mapped_regs: usize) -> Result<Self, i32> {
        info!("New E1000 device @ {:#x}", mapped_regs);
        // 分配的ring内存空间需要16字节对齐
        let alloc_tx_ring_pages =
            ((TX_RING_SIZE * size_of::<TxDesc>()) + (K::PAGE_SIZE - 1)) / K::PAGE_SIZE;
        let alloc_rx_ring_pages =
            ((RX_RING_SIZE * size_of::<RxDesc>()) + (K::PAGE_SIZE - 1)) / K::PAGE_SIZE;

        // Exercise3 Checkpoint 1
        // 分配tx_ring和rx_ring的内存空间并返回dma虚拟地址和物理地址
        let (tx_ring_vaddr, tx_ring_dma) = kfn.dma_alloc_coherent(alloc_tx_ring_pages);
        let (rx_ring_vaddr, rx_ring_dma) = kfn.dma_alloc_coherent(alloc_rx_ring_pages);


        let tx_ring = unsafe { from_raw_parts_mut(tx_ring_vaddr as *mut TxDesc, TX_RING_SIZE) };
        let rx_ring = unsafe { from_raw_parts_mut(rx_ring_vaddr as *mut RxDesc, RX_RING_SIZE) };

        tx_ring.fill(TxDesc {
            addr: 0,
            length: 0,
            cso: 0,
            cmd: 0,
            status: 0,
            css: 0,
            special: 0,
        });
        rx_ring.fill(RxDesc {
            addr: 0,
            length: 0,
            csum: 0,
            status: 0,
            errors: 0,
            special: 0,
        });

        let mut tx_mbufs = Vec::with_capacity(tx_ring.len());
        let mut rx_mbufs = Vec::with_capacity(rx_ring.len());

        let alloc_tx_buffer_pages = ((TX_RING_SIZE * MBUF_SIZE) + (K::PAGE_SIZE - 1)) / K::PAGE_SIZE;
        let alloc_rx_buffer_pages = ((RX_RING_SIZE * MBUF_SIZE) + (K::PAGE_SIZE - 1)) / K::PAGE_SIZE;

        // Exercise3 Checkpoint 2
        // 分配tx_buffer和rx_buffer的内存空间 并返回dma虚拟地址和物理地址
        let (mut tx_mbufs_vaddr, mut tx_mbufs_dma) =kfn.dma_alloc_coherent(alloc_tx_buffer_pages);
        let (mut rx_mbufs_vaddr, mut rx_mbufs_dma) =kfn.dma_alloc_coherent(alloc_rx_buffer_pages);

        

        if rx_mbufs_vaddr == 0 {
            panic!("e1000, alloc dma rx buffer failed");
        }


        for i in 0..TX_RING_SIZE {
            tx_ring[i].status = E1000_TXD_STAT_DD as u8;
            tx_ring[i].addr = tx_mbufs_dma as u64;
            tx_mbufs.push(tx_mbufs_vaddr);
            tx_mbufs_dma += MBUF_SIZE;
            tx_mbufs_vaddr += MBUF_SIZE;
        }


        for i in 0..RX_RING_SIZE {
            rx_ring[i].addr = rx_mbufs_dma as u64;
            rx_mbufs.push(rx_mbufs_vaddr);
            rx_mbufs_dma += MBUF_SIZE;
            rx_mbufs_vaddr += MBUF_SIZE;
        }

        // Slice切片，内存連續的動態大小的序列；
        // array, 数组
        // Vec, 内存連續的可增長數組類型

        // 寄存器读写。写自己存器时，先写一遍，再读一遍，确保值写成功。
        // ptr::write_volatile
        // ptr::read_volatile

        // 处理网络包的部分头字段
        // impl<T: AsRef<[u8]> + AsMut<[u8]>> Packet<T>
        // 或？ vcell::VolatileCell

        /* volatile = "0.4.5"
        let regs = unsafe{ from_raw_parts_mut(mapped_regs as *mut u32, len) };
        let regs = Volatile::new(regs);
        regs.index_mut(E1000_IMS).write(0);

        #[repr(transparent)]
        只能用于只有单个非零大小字段（可能还有其他零大小字段，如PhantomData<T>）的
        struct或enum 中。使得整个结构的内存布局和ABI被保证与该非零字段相同。
        */
        // 0x00000 ~ 0x1FFFF, I/O-Mapped Internal Registers and Memories
        let len = 0x1FFFF / size_of::<u32>();
        // 处理网卡寄存器配置: 由一个指针和一个长度len形成一个slice切片。len是元素的个数，而非字节数。
        let regs = unsafe { from_raw_parts_mut(mapped_regs as *mut Volatile<u32>, len) };

        let mut e1000dev = E1000Device {
            regs,
            rx_ring_dma,
            tx_ring_dma,
            rx_ring,
            tx_ring,
            rx_mbufs,
            tx_mbufs,
            mbuf_size: MBUF_SIZE,
            kfn,
        };
        e1000dev.e1000_init();

        Ok(e1000dev)
    }

    /// Initialize e1000 driver  
    /// mapped_regs is the memory address at which the e1000's registers are mapped.
    pub fn e1000_init(&mut self) {
        let stat = self.regs[E1000_STAT].read();
        let ctl = self.regs[E1000_CTL].read();
        info!("e1000 CTL: {:#x}, Status: {:#x}", ctl, stat);

        // Reset the device
        self.regs[E1000_IMS].write(0); // disable interrupts
        self.regs[E1000_CTL].write(ctl | E1000_CTL_RST);
        self.regs[E1000_IMS].write(0); // redisable interrupts

        // 内存壁垒 fence
        //__sync_synchronize();
        fence_w();

        // [E1000 14.5] Transmit initialization
        if (self.tx_ring.len() * size_of::<TxDesc>()) % 128 != 0 {
            //panic("e1000");
            error!("e1000, size of tx_ring is invalid");
        }

        // Exercise3 Checkpoint 3
        // set tx descriptor base address and tx ring length
        self.regs[E1000_TDBAL].write(self.tx_ring_dma as u32);
        self.regs[E1000_TDLEN].write((self.tx_ring.len() * size_of::<TxDesc>()) as u32);




        self.regs[E1000_TDT].write(0);
        self.regs[E1000_TDH].write(0);

        // [E1000 14.4] Receive initialization
        if (self.rx_ring.len() * size_of::<RxDesc>()) % 128 != 0 {
            error!("e1000, size of rx_ring is invalid");
        }

        // Exercise3 Checkpoint 4
        // set rx descriptor base address and rx ring length
        self.regs[E1000_RDBAL].write(self.rx_ring_dma as u32);
        self.regs[E1000_RDLEN].write((self.rx_ring.len() * size_of::<RxDesc>()) as u32);

        self.regs[E1000_RDH].write(0);
        self.regs[E1000_RDT].write((RX_RING_SIZE - 1) as u32);
        


        // filter by qemu's MAC address, 52:54:00:12:34:56
        self.regs[E1000_RA].write(0x12005452);
        self.regs[E1000_RA + 1].write(0x5534 | (1 << 31));
        // multicast table
        for i in 0..(4096 / 32) {
            self.regs[E1000_MTA + i].write(0);
        }


        // transmitter control bits.
        self.regs[E1000_TCTL].write(
            E1000_TCTL_EN |  // enable
            E1000_TCTL_PSP |                  // pad short packets
            (0x10 << E1000_TCTL_CT_SHIFT) |   // collision stuff
            (0x40 << E1000_TCTL_COLD_SHIFT),
        );
        self.regs[E1000_TIPG].write(10 | (8 << 10) | (6 << 20)); // inter-pkt gap

        // receiver control bits.
        self.regs[E1000_RCTL].write(
            E1000_RCTL_EN | // enable receiver
            E1000_RCTL_BAM |                 // enable broadcast
            E1000_RCTL_SZ_2048 |             // 2048-byte rx buffers
            E1000_RCTL_SECRC,
        ); // strip CRC

        self.regs[E1000_TIDV].write(0);
        self.regs[E1000_TADV].write(0);
        // ask e1000 for receive interrupts.
        self.regs[E1000_RDTR].write(0); // interrupt after every received packet (no timer)
        self.regs[E1000_RADV].write(0); // interrupt after every packet (no timer)
        self.regs[E1000_IMS].write(1 << 7); // RXT0 - Receiver Timer Interrupt , RXDW -- Receiver Descriptor Write Back

        self.regs[E1000_ICR].read(); // clear ints
        self.e1000_write_flush();
        info!("e1000_init has been completed");
    }

    /// Transmitting network packets
    pub fn e1000_transmit(&mut self, packet: &[u8]) -> i32 {
        let tindex = self.regs[E1000_TDT].read() as usize;
        info!("Read E1000_TDT = {:#x}", tindex);
        if (self.tx_ring[tindex].status & E1000_TXD_STAT_DD as u8) == 0 {
            error!("E1000 hasn't finished the corresponding previous transmission request");
            return -1;
        }

        let mut length = packet.len();
        if length > self.mbuf_size {
            error!("The packet: {} to be send is TOO LARGE", length);
            length = min(length, self.mbuf_size);
        }

        let mbuf = unsafe { from_raw_parts_mut(self.tx_mbufs[tindex] as *mut u8, length) };
        mbuf.copy_from_slice(packet);

        info!(">>>>>>>>> TX PKT {}", length);
        info!("\n\r");
        //print_hex_dump(tx_mbuf, 64);

        self.tx_ring[tindex].length = length as u16;
        self.tx_ring[tindex].status = 0;
        self.tx_ring[tindex].cmd = (E1000_TXD_CMD_RS | E1000_TXD_CMD_EOP) as u8;

        self.regs[E1000_TDT].write(((tindex + 1) % TX_RING_SIZE) as u32);

        self.e1000_write_flush();
        // sync
        fence_w();

        length as i32
    }

    // Todo: send and recv lock
    /// Receiving network packets
    pub fn e1000_recv(&mut self) -> Option<Vec<Vec<u8>>> {
        // Check for packets that have arrived from the e1000
        // Create and deliver an mbuf for each packet (using net_rx()).
        //let mut recv_packets = VecDeque::new();
        let mut recv_packets = Vec::new();
        let mut rindex = (self.regs[E1000_RDT].read() as usize + 1) % RX_RING_SIZE;
        // DD设为1时，内存中的接收包是完整的
        while (self.rx_ring[rindex].status & E1000_RXD_STAT_DD as u8) != 0 {
            info!("Read E1000_RDT + 1 = {:#x}", rindex);
            let len = self.rx_ring[rindex].length as usize;
            let mbuf = unsafe { from_raw_parts_mut(self.rx_mbufs[rindex] as *mut u8, len) };
            info!("RX PKT {} <<<<<<<<<", len);
            //recv_packets.push_back(mbuf.to_vec());
            recv_packets.push(mbuf.to_vec());

            // Deliver the mbuf to the network stack
            net_rx(mbuf);

            fence();
            // Just need to clear 64 bits header
            mbuf[..min(64, len)].fill(0);

            self.rx_ring[rindex].status = 0;
            self.regs[E1000_RDT].write(rindex as u32);

            self.e1000_write_flush();
            // sync
            fence_w();

            rindex = (rindex + 1) % RX_RING_SIZE;
        }
        info!("e1000_recv\n\r");

        if recv_packets.len() > 0 {
            Some(recv_packets)
        } else {
            None
        }
    }
    
    // 参考
    // xv6_for_internet_os
    // https://xiayingp.gitbook.io/build_a_os/labs/lab-10-networking-part-1
    // https://blog.mky.moe/mit6828/10-lab10/

    /// Clear Interrupt
    pub fn e1000_irq_disable(&mut self) {
        self.regs[E1000_IMC].write(!0);
        self.e1000_write_flush();
    }

    /// Enable Interrupts
    pub fn e1000_irq_enable(&mut self) {
        self.regs[E1000_IMS].write(IMS_ENABLE_MASK);
        self.e1000_write_flush();
    }

    /// flush e1000 status
    pub fn e1000_write_flush(&mut self) {
        self.regs[E1000_STAT].read();
    }

    /// Cause a link status change interrupt
    pub fn e1000_cause_lsc_int(&mut self) {
        self.regs[E1000_ICS].write(E1000_ICR_LSC);
    }

    /// To handle e1000 interrupt
    pub fn e1000_intr(&mut self) -> u32 {
        //self.e1000_recv();

        // tell the e1000 we've seen this interrupt;
        // without this the e1000 won't raise any
        // further interrupts.
        self.regs[E1000_ICR].read()
    }
}

/// called by e1000 driver's interrupt handler to deliver a packet to the
/// networking stack
pub fn net_rx(packet: &mut [u8]) {
    /*
    struct eth *ethhdr;
    uint16 type;

    ethhdr = mbufpullhdr(m, *ethhdr);
    if (!ethhdr) {
      mbuffree(m);
      return;
    }

    type = ntohs(ethhdr->type);
    if (type == ETHTYPE_IP)
      net_rx_ip(m);
    else if (type == ETHTYPE_ARP)
      net_rx_arp(m);
    else
      mbuffree(m);

      */
}
