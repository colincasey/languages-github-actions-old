require('../../bootstrap').invokeWith(({ getInput }) => {
    return [
        'prepare',
        '--bump',
        getInput('bump', { required: true }),
        getInput('project_dir')
    ]
})