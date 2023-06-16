require('../../bootstrap').invokeWith(({ getInput }) => {
    return [
        'prepare-release',
        
        '--bump',
        getInput('bump', { required: true }),

        '--repository-url',
        getInput('repository-url')
    ]
})