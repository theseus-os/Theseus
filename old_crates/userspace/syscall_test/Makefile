USERSPACE_BUILD_DIR ?= ../build

.PHONY: all clean
.DEFAULT_GOAL := all


all: syscall_send.s syscall_receive.s
	@mkdir -p $(USERSPACE_BUILD_DIR)

	@nasm -f elf64 syscall_receive.s -o $(USERSPACE_BUILD_DIR)/syscall_receive.o
	@ld -o $(USERSPACE_BUILD_DIR)/syscall_receive $(USERSPACE_BUILD_DIR)/syscall_receive.o
	@rm $(USERSPACE_BUILD_DIR)/syscall_receive.o
	
	@nasm -f elf64 syscall_send.s -o $(USERSPACE_BUILD_DIR)/syscall_send.o
	@ld -o $(USERSPACE_BUILD_DIR)/syscall_send $(USERSPACE_BUILD_DIR)/syscall_send.o
	@rm $(USERSPACE_BUILD_DIR)/syscall_send.o

clean:
	@rm -rf $(USERSPACE_BUILD_DIR)/syscall_send* \
		$(USERSPACE_BUILD_DIR)/syscall_receive*
