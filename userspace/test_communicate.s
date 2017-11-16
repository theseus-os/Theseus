main:
	pushq	rbp
	movq	rsp, rbp
	movl	0, -4(rbp)
	addl	1, -4(rbp)
	movl	-4(rbp), eax
	popq	rbp
	ret
