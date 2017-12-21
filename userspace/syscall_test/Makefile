USERSPACE_BUILD_DIR ?= build

all: syscall_send.s syscall_receive.s
	@mkdir -p ../$(USERSPACE_BUILD_DIR)

	@nasm -f elf64 syscall_receive.s -o ../$(USERSPACE_BUILD_DIR)/syscall_receive.o
	@ld -o ../$(USERSPACE_BUILD_DIR)/syscall_receive ../$(USERSPACE_BUILD_DIR)/syscall_receive.o
	
	@nasm -f elf64 syscall_send.s -o ../$(USERSPACE_BUILD_DIR)/syscall_send.o
	@ld -o ../$(USERSPACE_BUILD_DIR)/syscall_send ../$(USERSPACE_BUILD_DIR)/syscall_send.o

clean:
	@rm -rf build
