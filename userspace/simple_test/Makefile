all: test_program.s
	@mkdir -p ../$(USERSPACE_BUILD_DIR)
	@nasm -f bin test_program.s -o ../$(USERSPACE_BUILD_DIR)/test_program


clean:
	@rm -rf build
